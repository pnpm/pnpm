use std::sync::Arc;

use node_semver::Version;
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use pnpm_package_manager::{PickPolicy, create_configured_registry_resolver};
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
    selectors::{
        EngineSelectorPlan, build_selector_plan, match_engine_selector, remaining_regular_packages,
        should_update_engine_dep,
    },
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

    let remaining_packages = remaining_regular_packages(opts.packages, &selector_plan);

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
