pub(crate) mod cargo;
pub(crate) mod cargo_manifest_ops;
pub(crate) mod python;
pub(crate) mod selectors;
pub(crate) mod standalone;

pub(crate) use outdated::{
    collect_ecosystem_outdated,
    declared_ecosystem_dep_names,
    ecosystem_project_name,
    requires_ecosystem_lockfile,
};
pub(crate) use selectors::{
    EcosystemFilterKind,
    excludes_only,
    is_ecosystem_prefixed,
    patterns_for_ecosystem,
    validate_ecosystem_selectors,
};
pub(crate) use standalone::{
    select_recursive_and_standalone_projects,
    select_standalone_ecosystem_dirs,
};

mod outdated;

use crate::ecosystem_install::InstallContext;
use cargo::{
    CargoUpdateOptions,
    update_cargo_projects,
    validate_explicit_cargo_requests,
};
use miette::Result;
use pnpm_config::Config;
use pnpm_matcher::{
    Matcher,
    create_matcher,
};
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::{
    DependencyGroup,
    PackageManifest,
};
use pnpm_reporter::Reporter;
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;
use python::{
    PythonUpdateOptions,
    PythonUpdateTargets,
    resolve_python_update_targets,
    update_python_projects,
    validate_explicit_python_requests,
};
use selectors::{
    parse_update_selectors,
    updates_all_except_exclusions,
};
use std::{
    collections::{
        BTreeSet,
        HashSet,
    },
    path::PathBuf,
    sync::Arc,
};

pub(crate) struct EcosystemUpdateStepOptions<'a> {
    pub(crate) selectors: &'a [String],
    pub(crate) include_groups: &'a [DependencyGroup],
    pub(crate) ignore_matcher: Option<&'a Matcher>,
    pub(crate) latest: bool,
    pub(crate) no_save: bool,
    pub(crate) lockfile_only: bool,
}

pub(crate) struct EcosystemUpdateOutcome {
    pub(crate) updated_any: bool,
    pub(crate) remaining_node_selectors: Vec<String>,
}

struct EcosystemUpdateContext<'a, 'options> {
    config: &'static Config,
    http_client: &'a Arc<ThrottledClient>,
    options: &'a EcosystemUpdateStepOptions<'options>,
    parsed: &'a selectors::ParsedUpdateSelectors,
}

pub(crate) async fn run_ecosystem_updates_for_selection<Reporter: self::Reporter + 'static>(
    state: &crate::State,
    selection: Option<&crate::cli_args::pipelines::InstallFamilySelection>,
    standalone_dirs: &[PathBuf],
    options: EcosystemUpdateStepOptions<'_>,
) -> Result<EcosystemUpdateOutcome> {
    let (target_dirs, manifests) = target_dirs_and_manifests(state, selection, standalone_dirs);
    Box::pin(run_ecosystem_updates_on_targets::<Reporter>(
        &target_dirs,
        &manifests,
        state.config,
        &state.http_client,
        options,
    ))
    .await
}

fn target_dirs_and_manifests<'a>(
    state: &'a crate::State,
    selection: Option<&'a crate::cli_args::pipelines::InstallFamilySelection>,
    standalone_dirs: &[PathBuf],
) -> (Vec<PathBuf>, Vec<&'a PackageManifest>) {
    let Some(selection) = selection else {
        return (vec![state.project_dir().to_path_buf()], vec![&state.manifest]);
    };
    let mut dirs = Vec::new();
    let mut manifests = Vec::new();

    for project in &selection.projects {
        if selection.selected_dirs.contains(&project.root_dir) {
            dirs.push(project.root_dir.clone());
            manifests.push(&project.manifest);
        }
    }
    dirs.extend_from_slice(standalone_dirs);
    (dirs, manifests)
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
    let python_targets = if selectors_can_match_python(options.selectors, &parsed) {
        resolve_python_update_targets(target_dirs, config).await?
    } else {
        PythonUpdateTargets::default()
    };
    validate_explicit_requests(target_dirs, &python_targets, config, &parsed)?;
    let all_groups = [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
    let python_dep_names = python_targets.declared_names_for_groups(&all_groups);
    let updated_any = update_ecosystem_targets::<Reporter>(
        target_dirs,
        python_targets,
        EcosystemUpdateContext { config, http_client, options: &options, parsed: &parsed },
    )
    .await?;
    let remaining_node_selectors = filter_remaining_node_selectors(
        target_dirs,
        &python_dep_names,
        manifests,
        config,
        &parsed.node_selectors,
    )?;
    Ok(EcosystemUpdateOutcome { updated_any, remaining_node_selectors })
}

fn selectors_can_match_python(
    selectors: &[String],
    parsed: &selectors::ParsedUpdateSelectors,
) -> bool {
    selectors.is_empty()
        || !parsed.python_requests.is_empty()
        || !parsed.python_patterns.is_empty()
        || updates_all_except_exclusions(selectors, parsed)
}

async fn update_ecosystem_targets<Reporter: self::Reporter + 'static>(
    target_dirs: &[PathBuf],
    python_targets: PythonUpdateTargets,
    run: EcosystemUpdateContext<'_, '_>,
) -> Result<bool> {
    let update_all_except_exclusions =
        updates_all_except_exclusions(run.options.selectors, run.parsed);
    let cargo_matcher =
        (!run.parsed.cargo_patterns.is_empty()).then(|| create_matcher(&run.parsed.cargo_patterns));
    let restrict_to_selectors = !run.options.selectors.is_empty();
    let cargo_opts = CargoUpdateOptions {
        requests: &run.parsed.cargo_requests,
        exclusions: &run.parsed.cargo_exclusions,
        update_all_except_exclusions,
        bare_matcher: cargo_matcher.as_ref(),
        restrict_to_selectors,
        step: run.options,
    };
    let python_matcher = (!run.parsed.python_patterns.is_empty()).then(|| {
        create_matcher(&run.parsed.python_patterns)
    });
    let py_opts = PythonUpdateOptions {
        requests: &run.parsed.python_requests,
        exclusions: &run.parsed.python_exclusions,
        update_all_except_exclusions,
        bare_matcher: python_matcher.as_ref(),
        restrict_to_selectors,
        step: run.options,
    };
    let install_context = InstallContext {
        config: run.config,
        http_client: Arc::clone(run.http_client),
        lockfile_only: run.options.lockfile_only,
        frozen_lockfile: false,
    };
    let cargo_updated =
        update_cargo_projects::<Reporter>(target_dirs, install_context.clone(), &cargo_opts).await?;
    let py_updated =
        update_python_projects::<Reporter>(python_targets, install_context, &py_opts).await?;
    Ok(cargo_updated || py_updated)
}

fn validate_explicit_requests(
    target_dirs: &[PathBuf],
    python_targets: &PythonUpdateTargets,
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
        validate_explicit_python_requests(python_targets, &parsed.python_requests)?;
    }
    Ok(())
}

fn filter_remaining_node_selectors(
    target_dirs: &[PathBuf],
    python_dep_names: &BTreeSet<String>,
    manifests: &[&PackageManifest],
    config: &Config,
    node_selectors: &[String],
) -> Result<Vec<String>> {
    if node_selectors
        .iter()
        .map(|selector| selector.trim_start_matches('!'))
        .all(|selector| selector.starts_with('@'))
    {
        return Ok(node_selectors.to_vec());
    }
    let cargo_names = collect_declared_cargo_names(target_dirs, config)?;
    if cargo_names.is_empty() && python_dep_names.is_empty() {
        return Ok(node_selectors.to_vec());
    }
    let node_deps = collect_declared_node_names(manifests);
    let remaining = node_selectors
        .iter()
        .filter(|selector| {
            let parsed = parse_wanted_dependency(selector);
            parsed.alias
                .as_deref()
                .is_none_or(|name| {
                    node_deps.contains(name)
                        || (!cargo_names.contains(name)
                            && !python_dep_names.contains(
                                &selectors::normalize_python_name_pattern(name),
                            ))
                })
        })
        .cloned()
        .collect();
    Ok(pnpm_matcher::prune_orphan_ignores(remaining, node_selectors))
}

fn collect_declared_cargo_names(
    target_dirs: &[PathBuf],
    config: &Config,
) -> Result<HashSet<String>> {
    let all_groups = [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
    let mut cargo_names = HashSet::new();
    if config.cargo.enabled {
        let workspace_root = config.workspace_dir.as_deref();
        for dir in target_dirs {
            cargo_names.extend(outdated::declared_cargo_dep_names(
                dir,
                workspace_root,
                &all_groups,
            )?);
        }
    }
    Ok(cargo_names)
}

fn collect_declared_node_names(manifests: &[&PackageManifest]) -> HashSet<String> {
    manifests
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
        .collect()
}

mod cargo_selection;
mod cargo_workspace_scope;
