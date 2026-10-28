use std::{collections::HashSet, sync::Arc};

use node_semver::Version;
use pnpm_config::Config;
use pnpm_matcher::{Matcher, create_matcher};
use pnpm_network::ThrottledClient;
use pnpm_package_manager::{
    ParsedSelector, PickPolicy, UpdateError, create_configured_registry_resolver,
    parse_update_param,
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_registry::RangeSpecStyle;
use pnpm_resolving_default_resolver::DefaultResolver;
use pnpm_resolving_resolver_base::{
    LatestQuery, ResolutionPolicyOptions, ResolveOptions, VersionSelectionOptions,
    WantedDependency as ResolverWantedDependency,
};
use pnpm_tool_protocol::{RUNTIME_PROTOCOL, strip_tool_protocol};
use serde_json::Value;

use super::{
    DeclaredEngineDep, EngineDepKind, declared_engine_deps,
    persist::{
        commit_manifest_engine_updates, compute_updated_engine_spec, record_resolved_engine_update,
    },
    pm_resolver_bare_specifier,
};
use crate::{cli_args::pipelines::InstallFamilySelection, state::State};

pub(crate) struct EngineUpdateOptions<'a> {
    pub(crate) include_direct: &'a [DependencyGroup],
    pub(crate) packages: &'a [String],
    pub(crate) latest: bool,
    pub(crate) no_save: bool,
    pub(crate) save_exact: bool,
    pub(crate) range_spec_style: RangeSpecStyle,
}

pub(crate) struct EngineUpdateOutcome {
    pub(crate) has_engine_deps: bool,
    pub(crate) remaining_packages: Vec<String>,
    pub(crate) has_regular_deps: bool,
}

impl EngineUpdateOutcome {
    pub(crate) fn should_run_package_update(
        &self,
        packages: &[String],
        patches: bool,
        updates_packages: bool,
    ) -> bool {
        if !packages.is_empty() {
            !self.remaining_packages.is_empty()
        } else if self.has_engine_deps && !self.has_regular_deps && !patches {
            false
        } else {
            updates_packages
        }
    }
}

struct EngineSelectorPlan<'a> {
    raw_packages: &'a [String],
    parsed_selectors: Vec<ParsedSelector>,
    matcher: Option<Matcher>,
    ignore_matcher: Option<Matcher>,
    matched_engine_indices: HashSet<usize>,
    regular_dep_indices: HashSet<usize>,
    has_regular_deps: bool,
}

pub(crate) async fn update_engine_deps(
    state: &mut State,
    selection: Option<&mut InstallFamilySelection>,
    opts: &EngineUpdateOptions<'_>,
) -> miette::Result<EngineUpdateOutcome> {
    let mut selector_plan = build_selector_plan(state, selection.as_deref(), opts.packages);
    let mut resolver_and_opts: Option<(DefaultResolver, ResolveOptions)> = None;
    let mut has_engine_deps = false;
    let config = state.config;
    let http_client = Arc::clone(&state.http_client);

    let mut manifests_to_update = selected_manifests_mut(&mut state.manifest, selection);
    for manifest in &mut manifests_to_update {
        let engine_deps = declared_engine_deps(manifest, config, opts.include_direct);
        if engine_deps.is_empty() {
            continue;
        }
        has_engine_deps = true;
        update_manifest_engines(
            manifest,
            engine_deps,
            config,
            &http_client,
            opts,
            &mut selector_plan,
            &mut resolver_and_opts,
        )
        .await?;
    }

    let remaining_packages = opts.packages
        .iter()
        .enumerate()
        .filter(|(idx, _)| {
            !selector_plan.matched_engine_indices.contains(idx)
                || selector_plan.regular_dep_indices.contains(idx)
        })
        .map(|(_, pkg)| pkg.clone())
        .collect();

    Ok(EngineUpdateOutcome {
        has_engine_deps,
        remaining_packages,
        has_regular_deps: selector_plan.has_regular_deps,
    })
}

fn selected_manifests_mut<'a>(
    root_manifest: &'a mut PackageManifest,
    selection: Option<&'a mut InstallFamilySelection>,
) -> Vec<&'a mut PackageManifest> {
    match selection {
        Some(sel) => sel.projects
            .iter_mut()
            .filter(|project| sel.selected_dirs.contains(&project.root_dir))
            .map(|project| &mut project.manifest)
            .collect(),
        None => vec![root_manifest],
    }
}

fn build_selector_plan<'a>(
    state: &State,
    selection: Option<&InstallFamilySelection>,
    packages: &'a [String],
) -> EngineSelectorPlan<'a> {
    let parsed_selectors: Vec<_> = packages
        .iter()
        .map(|spec| parse_update_param(spec))
        .collect();
    let patterns: Vec<String> = parsed_selectors
        .iter()
        .map(|sel| sel.pattern.clone())
        .collect();
    let matcher = (!patterns.is_empty()).then(|| create_matcher(&patterns));
    let ignore_matcher = state.config.update_config.ignore_dependencies
        .as_deref()
        .filter(|patterns| !patterns.is_empty())
        .map(create_matcher);
    let mut plan = EngineSelectorPlan {
        raw_packages: packages,
        parsed_selectors,
        matcher,
        ignore_matcher,
        matched_engine_indices: HashSet::new(),
        regular_dep_indices: HashSet::new(),
        has_regular_deps: false,
    };
    scan_regular_deps(state, selection, &mut plan);
    plan
}

fn scan_regular_deps(
    state: &State,
    selection: Option<&InstallFamilySelection>,
    plan: &mut EngineSelectorPlan<'_>,
) {
    match selection {
        Some(sel) => {
            for project in &sel.projects {
                if sel.selected_dirs.contains(&project.root_dir) {
                    scan_manifest_regular_deps(&project.manifest, plan);
                }
            }
        }
        None => scan_manifest_regular_deps(&state.manifest, plan),
    }
    if let Some(snapshots) = state.lockfile
        .get()
        .ok()
        .flatten()
        .and_then(|lf| lf.snapshots.as_ref())
    {
        for key in snapshots.keys() {
            record_matching_selectors(
                &key.name.to_string(),
                &plan.parsed_selectors,
                &mut plan.regular_dep_indices,
            );
        }
    }
}

fn scan_manifest_regular_deps(manifest: &PackageManifest, plan: &mut EngineSelectorPlan<'_>) {
    for (dep_name, _) in manifest.dependencies([
        DependencyGroup::Prod,
        DependencyGroup::Dev,
        DependencyGroup::Optional,
        DependencyGroup::Peer,
    ]) {
        plan.has_regular_deps = true;
        record_matching_selectors(dep_name, &plan.parsed_selectors, &mut plan.regular_dep_indices);
    }
}

fn record_matching_selectors(
    name: &str,
    selectors: &[ParsedSelector],
    matched: &mut HashSet<usize>,
) {
    for (idx, sel) in selectors.iter().enumerate() {
        if create_matcher(std::slice::from_ref(&sel.pattern)).matches(name) {
            matched.insert(idx);
        }
    }
}

async fn update_manifest_engines(
    manifest: &mut PackageManifest,
    engine_deps: Vec<DeclaredEngineDep>,
    config: &Config,
    http_client: &Arc<ThrottledClient>,
    opts: &EngineUpdateOptions<'_>,
    selector_plan: &mut EngineSelectorPlan<'_>,
    resolver_and_opts: &mut Option<(DefaultResolver, ResolveOptions)>,
) -> miette::Result<()> {
    let mut manifest_updates = Vec::new();
    let mut pnpm_sync_target: Option<(String, String)> = None;

    for dep in engine_deps {
        if let Some((resolved_version, new_spec)) = resolve_engine_dep_update(
            &dep,
            config,
            http_client,
            opts,
            selector_plan,
            resolver_and_opts,
        )
        .await?
        {
            record_resolved_engine_update(
                dep,
                resolved_version,
                new_spec,
                opts.no_save,
                &mut pnpm_sync_target,
                &mut manifest_updates,
            );
        }
    }

    commit_manifest_engine_updates(config, manifest, &manifest_updates, pnpm_sync_target).await
}

async fn resolve_engine_dep_update(
    dep: &DeclaredEngineDep,
    config: &Config,
    http_client: &Arc<ThrottledClient>,
    opts: &EngineUpdateOptions<'_>,
    selector_plan: &mut EngineSelectorPlan<'_>,
    resolver_and_opts: &mut Option<(DefaultResolver, ResolveOptions)>,
) -> miette::Result<Option<(Version, String)>> {
    if !should_update_engine_dep(dep, opts, selector_plan) {
        return Ok(None);
    }
    let requested = match_engine_selector(&dep.name, opts.latest, selector_plan)?;
    let (resolver, resolve_options) = match resolver_and_opts {
        Some(pair) => pair,
        None => resolver_and_opts.insert(build_engine_resolver(config, http_client)?),
    };
    let stripped_req = requested
        .as_deref()
        .map(|req| strip_tool_protocol(req).unwrap_or(req).trim());
    let (query_spec, compatible) = engine_latest_query(dep, stripped_req, opts.latest);
    let Some(resolved_version) =
        query_engine_version(resolver, resolve_options, dep, query_spec, compatible).await?
    else {
        return Ok(None);
    };
    if stripped_req.is_none()
        && dep.current
            .as_ref()
            .is_some_and(|current| resolved_version <= *current)
    {
        return Ok(None);
    }
    let new_spec = compute_updated_engine_spec(
        dep,
        stripped_req,
        &resolved_version,
        opts.save_exact,
        opts.range_spec_style,
    );
    Ok(Some((resolved_version, new_spec)))
}

fn should_update_engine_dep(
    dep: &DeclaredEngineDep,
    opts: &EngineUpdateOptions<'_>,
    selector_plan: &EngineSelectorPlan<'_>,
) -> bool {
    if opts.packages.is_empty()
        && selector_plan.ignore_matcher
            .as_ref()
            .is_some_and(|ignore| ignore.matches(&dep.name))
    {
        return false;
    }
    selector_plan.matcher
        .as_ref()
        .is_none_or(|matcher| matcher.matches(&dep.name))
}

fn match_engine_selector(
    dep_name: &str,
    latest: bool,
    selector_plan: &mut EngineSelectorPlan<'_>,
) -> miette::Result<Option<String>> {
    let mut requested: Option<String> = None;
    for (idx, sel) in selector_plan.parsed_selectors.iter().enumerate() {
        if create_matcher(std::slice::from_ref(&sel.pattern)).matches(dep_name) {
            selector_plan.matched_engine_indices.insert(idx);
            if requested.is_none() && !sel.pattern.starts_with('!') {
                requested.clone_from(&sel.version);
            }
        }
    }
    if latest && requested.is_some() {
        return Err(latest_with_spec_error(
            selector_plan.raw_packages,
            &selector_plan.parsed_selectors,
        ));
    }
    Ok(requested)
}

fn latest_with_spec_error(
    packages: &[String],
    parsed_selectors: &[ParsedSelector],
) -> miette::Report {
    let with_spec = packages
        .iter()
        .zip(parsed_selectors)
        .filter(|(_, selector)| selector.version.is_some())
        .map(|(raw, _)| raw.as_str())
        .collect::<Vec<_>>();
    miette::Report::new(UpdateError::LatestWithSpec(with_spec.join(", ")))
}

fn build_engine_resolver(
    config: &Config,
    http_client: &Arc<ThrottledClient>,
) -> miette::Result<(DefaultResolver, ResolveOptions)> {
    let policy = PickPolicy::from_config(config).map_err(miette::Report::new)?;
    let resolver = create_configured_registry_resolver(config, Arc::clone(http_client), &policy)
        .map_err(miette::Report::new)?;
    let opts = ResolveOptions {
        version: VersionSelectionOptions {
            default_tag: Some("latest".to_string()),
            ..Default::default()
        },
        policy: ResolutionPolicyOptions {
            published_by: policy.published_by,
            published_by_exclude: policy.published_by_exclude,
            ..Default::default()
        },
        ..ResolveOptions::default()
    };
    Ok((resolver, opts))
}

fn engine_latest_query(
    dep: &DeclaredEngineDep,
    stripped_req: Option<&str>,
    latest: bool,
) -> (String, bool) {
    match stripped_req {
        Some(req) => {
            let spec = match dep.kind {
                EngineDepKind::PackageManager { pm, .. } => pm_resolver_bare_specifier(pm, req),
                EngineDepKind::Runtime { .. } => format!("{RUNTIME_PROTOCOL}{req}"),
            };
            (spec, req != "latest")
        }
        None => (dep.resolver_bare_specifier.clone(), !latest),
    }
}

async fn query_engine_version(
    resolver: &DefaultResolver,
    resolve_options: &ResolveOptions,
    dep: &DeclaredEngineDep,
    query_spec: String,
    compatible: bool,
) -> miette::Result<Option<Version>> {
    let latest_info = resolver
        .resolve_latest(
            &LatestQuery {
                wanted_dependency: ResolverWantedDependency {
                    alias: Some(dep.name.clone()),
                    bare_specifier: Some(query_spec),
                    optional: Some(dep.group == DependencyGroup::Optional),
                    ..ResolverWantedDependency::default()
                },
                compatible,
            },
            resolve_options,
        )
        .await
        .map_err(|err| {
            let reason = pnpm_network::redact_url_credentials(&err.to_string());
            miette::miette!("{reason}")
        })?;

    Ok(latest_info
        .and_then(|info| info.latest_manifest)
        .and_then(|manifest| {
            manifest
                .get("version")
                .and_then(Value::as_str)
                .and_then(|ver| Version::parse(ver).ok())
        }))
}
