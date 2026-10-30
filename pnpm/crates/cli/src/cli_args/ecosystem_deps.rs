pub(crate) mod cargo;
pub(crate) mod cargo_manifest_ops;
pub(crate) mod python;
pub(crate) mod selectors;
pub(crate) mod standalone;

pub(crate) use selectors::{
    EcosystemFilterKind, patterns_for_ecosystem, validate_ecosystem_selectors,
};
pub(crate) use standalone::{
    select_recursive_and_standalone_projects, select_standalone_ecosystem_dirs,
};

use crate::{
    cli_args::outdated::{OutdatedPackage, OutdatedQuery, OutdatedRun},
    ecosystem_install::InstallContext,
};
use cargo::{
    CargoUpdateOptions, collect_cargo_outdated, has_cargo_lockfile, update_cargo_project,
    validate_explicit_cargo_requests,
};
use cargo_manifest_ops::{cargo_project_name, declared_cargo_deps, has_matching_cargo_deps};
use miette::Result;
use pnpm_config::Config;
use pnpm_matcher::{Matcher, create_matcher};
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::Reporter;
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use python::{
    PythonUpdateOptions, collect_python_outdated, declared_python_dep_names,
    has_matching_python_deps, has_python_lockfile, python_project_name, update_python_project,
    validate_explicit_python_requests,
};
use selectors::{is_cargo_prefixed, is_python_prefixed, parse_update_selectors};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) struct EcosystemUpdateStepOptions<'a> {
    pub(crate) selectors: &'a [String],
    pub(crate) include_groups: &'a [DependencyGroup],
    pub(crate) ignore_matcher: Option<&'a Matcher>,
    pub(crate) latest: bool,
    pub(crate) no_save: bool,
    pub(crate) save_exact: bool,
    pub(crate) save_prefix: Option<&'a str>,
    pub(crate) lockfile_only: bool,
}

pub(crate) struct EcosystemUpdateOutcome {
    pub(crate) updated_any: bool,
    pub(crate) remaining_node_selectors: Vec<String>,
}

pub(crate) fn requires_ecosystem_lockfile(
    project_dir: &Path,
    config: &Config,
    query: &OutdatedQuery<'_>,
    package_patterns: &[String],
) -> Result<bool> {
    let workspace_root = config.workspace_dir.as_deref();
    if config.cargo.enabled
        && !has_cargo_lockfile(project_dir, workspace_root)
        && let Some(cargo_matcher) =
            ecosystem_matcher(package_patterns, EcosystemFilterKind::Cargo)?
    {
        let cargo_query =
            OutdatedQuery { match_names: cargo_matcher.as_ref().or(query.match_names), ..*query };
        if has_matching_cargo_deps(project_dir, workspace_root, &cargo_query)? {
            return Ok(true);
        }
    }
    if config.python.enabled
        && !has_python_lockfile(project_dir, workspace_root)
        && let Some(py_matcher) = ecosystem_matcher(package_patterns, EcosystemFilterKind::Python)?
    {
        let py_query =
            OutdatedQuery { match_names: py_matcher.as_ref().or(query.match_names), ..*query };
        if has_matching_python_deps(project_dir, &py_query)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn declared_ecosystem_dep_names(
    project_dir: &Path,
    config: &Config,
    include: &[DependencyGroup],
) -> Result<Vec<String>> {
    let mut names = Vec::new();
    if config.cargo.enabled {
        let deps = declared_cargo_deps(project_dir, config.workspace_dir.as_deref())?;
        for dep in deps
            .into_iter()
            .filter(|dep| include.contains(&dep.group))
        {
            names.extend([dep.alias.clone(), format!("crate:{}", dep.alias)]);
            names.extend([dep.package_name.clone(), format!("crate:{}", dep.package_name)]);
        }
    }
    if config.python.enabled {
        for name in declared_python_dep_names(project_dir, include)? {
            names.extend([format!("pypi:{name}"), name]);
        }
    }
    Ok(names)
}

pub(crate) fn ecosystem_project_name(project_dir: &Path, config: &Config) -> Option<String> {
    if config.cargo.enabled
        && let Some(name) = cargo_project_name(project_dir)
    {
        return Some(name);
    }
    if config.python.enabled
        && let Some(name) = python_project_name(project_dir)
    {
        return Some(name);
    }
    None
}

pub(crate) async fn collect_ecosystem_outdated(
    project_dir: &Path,
    workspace_name: Option<String>,
    query: &OutdatedQuery<'_>,
    raw_packages: &[String],
    run: &OutdatedRun,
) -> Result<Vec<OutdatedPackage>> {
    let mut out = Vec::new();
    if run.config.cargo.enabled
        && let Some(cargo_matcher) = ecosystem_matcher(raw_packages, EcosystemFilterKind::Cargo)?
    {
        let cargo_query =
            OutdatedQuery { match_names: cargo_matcher.as_ref().or(query.match_names), ..*query };
        out.extend(
            collect_cargo_outdated(
                project_dir,
                workspace_name.clone(),
                &cargo_query,
                &run.config,
                &run.http_client,
            )
            .await?,
        );
    }
    if run.config.python.enabled
        && let Some(py_matcher) = ecosystem_matcher(raw_packages, EcosystemFilterKind::Python)?
    {
        let py_query =
            OutdatedQuery { match_names: py_matcher.as_ref().or(query.match_names), ..*query };
        out.extend(
            collect_python_outdated(
                project_dir,
                workspace_name,
                &py_query,
                &run.config,
                &run.http_client,
            )
            .await?,
        );
    }
    Ok(out)
}

fn ecosystem_matcher(
    raw_packages: &[String],
    kind: EcosystemFilterKind,
) -> Result<Option<Option<Matcher>>> {
    if raw_packages.is_empty() {
        return Ok(Some(None));
    }
    let patterns = patterns_for_ecosystem(raw_packages, kind)?;
    if patterns.is_empty() {
        return Ok(None);
    }
    Ok(Some(Some(create_matcher(&patterns))))
}

pub(crate) async fn run_ecosystem_updates_for_selection<Reporter: self::Reporter + 'static>(
    state: &crate::State,
    selection: Option<&crate::cli_args::pipelines::InstallFamilySelection>,
    options: EcosystemUpdateStepOptions<'_>,
) -> Result<EcosystemUpdateOutcome> {
    let (target_dirs, manifests) = target_dirs_and_manifests(state, selection).await?;
    Box::pin(run_ecosystem_updates_on_targets::<Reporter>(
        &target_dirs,
        &manifests,
        state.config,
        &state.http_client,
        options,
    ))
    .await
}

async fn target_dirs_and_manifests<'a>(
    state: &'a crate::State,
    selection: Option<&'a crate::cli_args::pipelines::InstallFamilySelection>,
) -> Result<(Vec<PathBuf>, Vec<&'a PackageManifest>)> {
    let Some(selection) = selection else {
        return Ok((vec![state.project_dir().to_path_buf()], vec![&state.manifest]));
    };
    let mut dirs = Vec::new();
    let mut manifests = Vec::new();
    let mut npm_dirs = HashSet::new();
    for project in &selection.projects {
        npm_dirs.insert(project.root_dir.clone());
        if selection.selected_dirs.contains(&project.root_dir) {
            dirs.push(project.root_dir.clone());
            manifests.push(&project.manifest);
        }
    }
    let standalone = select_standalone_ecosystem_dirs(
        &selection.workspace_root,
        state.project_dir(),
        &npm_dirs,
        &selection.selected_dirs,
        state.config,
    )
    .await?;
    dirs.extend(standalone.selected);
    Ok((dirs, manifests))
}

async fn run_ecosystem_updates_on_targets<Reporter: self::Reporter + 'static>(
    target_dirs: &[PathBuf],
    manifests: &[&PackageManifest],
    config: &'static Config,
    http_client: &Arc<ThrottledClient>,
    options: EcosystemUpdateStepOptions<'_>,
) -> Result<EcosystemUpdateOutcome> {
    let parsed = parse_update_selectors(options.selectors, config)?;
    if !config.cargo.enabled && !config.python.enabled {
        return Ok(EcosystemUpdateOutcome {
            updated_any: false,
            remaining_node_selectors: parsed.node_selectors,
        });
    }
    validate_explicit_requests(target_dirs, config, &parsed)?;
    let restrict = !options.selectors.is_empty();
    let bare_matcher =
        (!parsed.bare_patterns.is_empty()).then(|| create_matcher(&parsed.bare_patterns));
    let mut updated_any = false;
    for project_dir in target_dirs {
        let context = InstallContext {
            config,
            http_client: Arc::clone(http_client),
            lockfile_only: options.lockfile_only,
            frozen_lockfile: false,
        };
        updated_any |= update_project_ecosystems::<Reporter>(
            project_dir,
            context,
            &options,
            &parsed,
            bare_matcher.as_ref(),
            restrict,
        )
        .await?;
    }
    let remaining_node_selectors =
        filter_remaining_node_selectors(target_dirs, manifests, config, &parsed.node_selectors)?;
    Ok(EcosystemUpdateOutcome { updated_any, remaining_node_selectors })
}

fn validate_explicit_requests(
    target_dirs: &[PathBuf],
    config: &Config,
    parsed: &selectors::ParsedUpdateSelectors,
) -> Result<()> {
    if config.cargo.enabled {
        validate_explicit_cargo_requests(
            target_dirs,
            config.workspace_dir.as_deref(),
            &parsed.cargo_requests,
        )?;
    }
    if config.python.enabled {
        validate_explicit_python_requests(target_dirs, &parsed.python_requests)?;
    }
    Ok(())
}

async fn update_project_ecosystems<Reporter: self::Reporter + 'static>(
    project_dir: &Path,
    context: InstallContext,
    options: &EcosystemUpdateStepOptions<'_>,
    parsed: &selectors::ParsedUpdateSelectors,
    bare_matcher: Option<&Matcher>,
    restrict_to_selectors: bool,
) -> Result<bool> {
    let cargo_opts = CargoUpdateOptions {
        requests: &parsed.cargo_requests,
        bare_matcher,
        restrict_to_selectors,
        step: options,
    };
    let cargo_updated =
        update_cargo_project::<Reporter>(project_dir, context.clone(), &cargo_opts).await?;
    let py_opts = PythonUpdateOptions {
        requests: &parsed.python_requests,
        bare_matcher,
        restrict_to_selectors,
        step: options,
    };
    let py_updated = update_python_project::<Reporter>(project_dir, context, &py_opts).await?;
    Ok(cargo_updated || py_updated)
}

fn filter_remaining_node_selectors(
    target_dirs: &[PathBuf],
    manifests: &[&PackageManifest],
    config: &Config,
    node_selectors: &[String],
) -> Result<Vec<String>> {
    if node_selectors.is_empty() {
        return Ok(Vec::new());
    }
    let all_groups = [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
    let mut eco_names = HashSet::new();
    for dir in target_dirs {
        eco_names.extend(declared_ecosystem_dep_names(dir, config, &all_groups)?);
    }
    if eco_names.is_empty() {
        return Ok(node_selectors.to_vec());
    }
    let node_deps: HashSet<String> = manifests
        .iter()
        .flat_map(|manifest| {
            manifest.dependencies([
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
                DependencyGroup::Peer,
            ])
        })
        .map(|(name, _)| name.to_string())
        .collect();
    Ok(node_selectors
        .iter()
        .filter(|selector| {
            if is_cargo_prefixed(selector) || is_python_prefixed(selector) {
                return false;
            }
            let parsed = parse_wanted_dependency(selector);
            let Some(name) = parsed.alias.as_deref() else {
                return true;
            };
            node_deps.contains(name) || !eco_names.contains(name)
        })
        .cloned()
        .collect())
}

mod cargo_selection;
mod cargo_workspace_scope;
