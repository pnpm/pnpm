pub(crate) mod cargo;
pub(crate) mod cargo_manifest_ops;
pub(crate) mod python;
pub(crate) mod selectors;

pub(crate) use selectors::{
    EcosystemFilterKind, patterns_for_ecosystem, validate_ecosystem_selectors,
};

use crate::{
    cli_args::outdated::{OutdatedPackage, OutdatedQuery, OutdatedRun},
    ecosystem_install::{EcosystemManifest, EcosystemWorkspaceInventory, InstallContext},
};
use cargo::{CargoUpdateOptions, collect_cargo_outdated, has_cargo_lockfile, update_cargo_project};
use cargo_manifest_ops::{cargo_project_name, declared_cargo_deps};
use miette::Result;
use pnpm_config::Config;
use pnpm_matcher::{Matcher, create_matcher};
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::Reporter;
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use python::{
    PythonUpdateOptions, collect_python_outdated, declared_python_dep_names, has_python_lockfile,
    python_project_name, update_python_project,
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

pub(crate) fn ecosystem_lockfile_state(
    project_dir: &Path,
    config: &Config,
) -> Result<(bool, bool)> {
    let workspace_root = config.workspace_dir.as_deref();
    let (cargo_declared, cargo_locked) = if config.cargo.enabled {
        let has_deps = !declared_cargo_deps(project_dir, workspace_root)?.is_empty();
        (has_deps, has_cargo_lockfile(project_dir, workspace_root))
    } else {
        (false, true)
    };
    let (py_declared, py_locked) = if config.python.enabled {
        let has_deps = !declared_python_dep_names(project_dir)?.is_empty();
        (has_deps, has_python_lockfile(project_dir, workspace_root))
    } else {
        (false, true)
    };
    let has_declared = cargo_declared || py_declared;
    let all_locked = (!cargo_declared || cargo_locked) && (!py_declared || py_locked);
    Ok((has_declared, has_declared && all_locked))
}

pub(crate) fn declared_ecosystem_dep_names(
    project_dir: &Path,
    config: &Config,
) -> Result<Vec<String>> {
    let mut names = Vec::new();
    if config.cargo.enabled {
        for dep in declared_cargo_deps(project_dir, config.workspace_dir.as_deref())? {
            names.push(dep.alias.clone());
            names.push(format!("crate:{}", dep.alias));
            if dep.package_name != dep.alias {
                names.push(dep.package_name.clone());
                names.push(format!("crate:{}", dep.package_name));
            }
        }
    }
    if config.python.enabled {
        for name in declared_python_dep_names(project_dir)? {
            names.push(format!("pypi:{name}"));
            names.push(name);
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

pub(crate) async fn discover_standalone_ecosystem_dirs(
    workspace_root: &Path,
    existing_dirs: &HashSet<PathBuf>,
    config: &Config,
) -> Result<Vec<PathBuf>> {
    if !config.cargo.enabled && !config.python.enabled {
        return Ok(Vec::new());
    }
    let inventory = EcosystemWorkspaceInventory::new(workspace_root.to_path_buf(), config);
    let mut dirs = Vec::new();
    let mut seen = existing_dirs.clone();
    if config.cargo.enabled {
        extend_manifest_dirs(
            inventory.manifests(EcosystemManifest::Cargo).await?,
            &mut seen,
            &mut dirs,
        );
    }
    if config.python.enabled {
        extend_manifest_dirs(
            inventory.manifests(EcosystemManifest::Python).await?,
            &mut seen,
            &mut dirs,
        );
    }
    Ok(dirs)
}

fn extend_manifest_dirs(
    manifests: &[PathBuf],
    seen: &mut HashSet<PathBuf>,
    dirs: &mut Vec<PathBuf>,
) {
    for manifest in manifests {
        if let Some(dir) = manifest.parent()
            && seen.insert(dir.to_path_buf())
        {
            dirs.push(dir.to_path_buf());
        }
    }
}

pub(crate) async fn run_ecosystem_updates<Reporter: self::Reporter + 'static>(
    project_dir: &Path,
    manifest: &PackageManifest,
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
    let restrict = !options.selectors.is_empty();
    let bare_matcher =
        (!parsed.bare_patterns.is_empty()).then(|| create_matcher(&parsed.bare_patterns));
    let context = InstallContext {
        config,
        http_client: Arc::clone(http_client),
        lockfile_only: options.lockfile_only,
        frozen_lockfile: false,
    };
    let cargo_updated = run_cargo_half::<Reporter>(
        project_dir,
        context.clone(),
        &options,
        &parsed,
        bare_matcher.as_ref(),
        restrict,
    )
    .await?;
    let python_updated = run_python_half::<Reporter>(
        project_dir,
        context,
        &options,
        &parsed,
        bare_matcher.as_ref(),
        restrict,
    )
    .await?;
    let remaining_node_selectors =
        filter_remaining_node_selectors(project_dir, manifest, config, &parsed.node_selectors)?;
    Ok(EcosystemUpdateOutcome {
        updated_any: cargo_updated || python_updated,
        remaining_node_selectors,
    })
}

pub(crate) async fn run_ecosystem_updates_for_selection<Reporter: self::Reporter + 'static>(
    state: &crate::State,
    selection: Option<&crate::cli_args::pipelines::InstallFamilySelection>,
    options: EcosystemUpdateStepOptions<'_>,
) -> Result<EcosystemUpdateOutcome> {
    let Some(selection) = selection else {
        return Box::pin(run_ecosystem_updates::<Reporter>(
            state.project_dir(),
            &state.manifest,
            state.config,
            &state.http_client,
            options,
        ))
        .await;
    };
    let mut updated_any = false;
    let mut remaining = options.selectors.to_vec();
    for project in &selection.projects {
        if !selection.selected_dirs.contains(&project.root_dir) {
            continue;
        }
        let out = Box::pin(run_ecosystem_updates::<Reporter>(
            &project.root_dir,
            &project.manifest,
            state.config,
            &state.http_client,
            EcosystemUpdateStepOptions { selectors: &remaining, ..options },
        ))
        .await?;
        updated_any |= out.updated_any;
        remaining = out.remaining_node_selectors;
    }
    Ok(EcosystemUpdateOutcome { updated_any, remaining_node_selectors: remaining })
}

async fn run_cargo_half<Reporter: self::Reporter + 'static>(
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
    update_cargo_project::<Reporter>(project_dir, context, &cargo_opts).await
}

async fn run_python_half<Reporter: self::Reporter + 'static>(
    project_dir: &Path,
    context: InstallContext,
    options: &EcosystemUpdateStepOptions<'_>,
    parsed: &selectors::ParsedUpdateSelectors,
    bare_matcher: Option<&Matcher>,
    restrict_to_selectors: bool,
) -> Result<bool> {
    let py_opts = PythonUpdateOptions {
        requests: &parsed.python_requests,
        bare_matcher,
        restrict_to_selectors,
        step: options,
    };
    update_python_project::<Reporter>(project_dir, context, &py_opts).await
}

fn filter_remaining_node_selectors(
    project_dir: &Path,
    manifest: &PackageManifest,
    config: &Config,
    node_selectors: &[String],
) -> Result<Vec<String>> {
    if node_selectors.is_empty() {
        return Ok(Vec::new());
    }
    let eco_names: HashSet<String> =
        declared_ecosystem_dep_names(project_dir, config)?.into_iter().collect();
    if eco_names.is_empty() {
        return Ok(node_selectors.to_vec());
    }
    let node_deps: HashSet<String> = manifest
        .dependencies([
            DependencyGroup::Prod,
            DependencyGroup::Dev,
            DependencyGroup::Optional,
            DependencyGroup::Peer,
        ])
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
