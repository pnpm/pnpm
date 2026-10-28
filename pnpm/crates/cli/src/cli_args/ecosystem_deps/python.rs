use super::{EcosystemUpdateStepOptions, selectors::PythonUpdateSelector};
use crate::{
    cli_args::outdated::{
        OutdatedPackage, OutdatedQuery, TargetVersion,
        query::{OutdatedDisplayVersions, OutdatedMetadata},
    },
    ecosystem_install::{EcosystemWorkspaceInventory, InstallContext, python as eco_python},
};
use miette::{Result, bail};
use pep440_rs::Version as Pep440Version;
use pnpm_config::Config;
use pnpm_install_coordinator::InstallPlan;
use pnpm_matcher::Matcher;
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::DependencyGroup;
use pnpm_python_installer::{
    DeclaredPythonDep, PackageName, PythonDependencyGroup, UpdateOptions, declared_dependencies,
    locked_versions, lockfile_dir_for, plan_update, project_name, resolve_target_version,
};
use pnpm_reporter::Reporter;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    str::FromStr,
};

pub(crate) struct PythonUpdateOptions<'a> {
    pub(crate) requests: &'a [PythonUpdateSelector],
    pub(crate) bare_matcher: Option<&'a Matcher>,
    pub(crate) restrict_to_selectors: bool,
    pub(crate) step: &'a EcosystemUpdateStepOptions<'a>,
}

pub(crate) fn python_project_name(project_dir: &Path) -> Option<String> {
    project_name(&project_dir.join("pyproject.toml"))
}

pub(crate) fn has_matching_python_deps(
    project_dir: &Path,
    query: &OutdatedQuery<'_>,
) -> Result<bool> {
    let pyproject = project_dir.join("pyproject.toml");
    if !pyproject.is_file() {
        return Ok(false);
    }
    Ok(declared_dependencies(&pyproject)?
        .into_iter()
        .any(|dep| {
            query.include_direct.contains(&map_python_group(dep.group))
                && query.matches_name(dep.name.as_ref())
        }))
}

pub(crate) fn has_python_lockfile(project_dir: &Path, workspace_root: Option<&Path>) -> bool {
    lockfile_dir_for(workspace_root, project_dir).join("pylock.toml").is_file()
}

pub(crate) fn declared_python_dep_names(project_dir: &Path) -> Result<Vec<String>> {
    Ok(declared_dependencies(&project_dir.join("pyproject.toml"))?
        .into_iter()
        .map(|dep| dep.name.to_string())
        .collect())
}

pub(crate) async fn collect_python_outdated(
    project_dir: &Path,
    workspace_name: Option<String>,
    query: &OutdatedQuery<'_>,
    config: &Config,
    http_client: &ThrottledClient,
) -> Result<Vec<OutdatedPackage>> {
    if !config.python.enabled {
        return Ok(Vec::new());
    }
    let pyproject_path = project_dir.join("pyproject.toml");
    let declared = declared_dependencies(&pyproject_path)?;
    if declared.is_empty() {
        return Ok(Vec::new());
    }
    let pylock_path =
        lockfile_dir_for(config.workspace_dir.as_deref(), project_dir).join("pylock.toml");
    if !pylock_path.is_file() {
        return Ok(Vec::new());
    }
    let locked = locked_versions(&pylock_path)?;
    let mut outdated = Vec::new();
    for dep in declared {
        let group = map_python_group(dep.group);
        let name_str = dep.name.to_string();
        if !query.include_direct.contains(&group) || !query.matches_name(&name_str) {
            continue;
        }
        if let Some(pkg) = resolve_outdated_python_dep(
            &dep,
            group,
            &locked,
            workspace_name.as_deref(),
            query,
            config,
            http_client,
        )
        .await?
        {
            outdated.push(pkg);
        }
    }
    Ok(outdated)
}

fn map_python_group(group: PythonDependencyGroup) -> DependencyGroup {
    match group {
        PythonDependencyGroup::Prod => DependencyGroup::Prod,
        PythonDependencyGroup::Optional => DependencyGroup::Optional,
        PythonDependencyGroup::Dev => DependencyGroup::Dev,
    }
}

async fn resolve_outdated_python_dep(
    dep: &DeclaredPythonDep,
    group: DependencyGroup,
    locked: &BTreeMap<PackageName, Pep440Version>,
    workspace_name: Option<&str>,
    query: &OutdatedQuery<'_>,
    config: &Config,
    http_client: &ThrottledClient,
) -> Result<Option<OutdatedPackage>> {
    let Some(current) = locked.get(&dep.name) else {
        return Ok(None);
    };
    let wanted = resolve_target_version(config, http_client, &dep.requirement, true)
        .await?
        .unwrap_or_else(|| current.clone());
    let target = match query.target_version {
        TargetVersion::WithinRange => wanted.clone(),
        TargetVersion::Latest => {
            let Some(latest) =
                resolve_target_version(config, http_client, &dep.requirement, false).await?
            else {
                return Ok(None);
            };
            latest
        }
    };
    if target <= *current {
        return Ok(None);
    }
    let name_str = dep.name.to_string();
    Ok(Some(OutdatedPackage {
        alias: name_str.clone(),
        package_name: name_str.clone(),
        belongs_to: group,
        current: pep440_to_node_version(current),
        target: pep440_to_node_version(&target),
        wanted: pep440_to_node_version(&wanted),
        github_action: false,
        metadata: OutdatedMetadata {
            deprecated: None,
            homepage: Some(format!("https://pypi.org/project/{name_str}/")),
            workspace: workspace_name.map(str::to_string),
            display_versions: Some(Box::new(OutdatedDisplayVersions {
                current: current.to_string(),
                target: target.to_string(),
                wanted: wanted.to_string(),
            })),
        },
    }))
}

fn pep440_to_node_version(version: &Pep440Version) -> node_semver::Version {
    let release = version.release();
    let major = release.first().copied().unwrap_or(0);
    let minor = release.get(1).copied().unwrap_or(0);
    let patch = release.get(2).copied().unwrap_or(0);
    node_semver::Version { major, minor, patch, build: Vec::new(), pre_release: Vec::new() }
}

pub(crate) async fn update_python_project<Reporter: self::Reporter + 'static>(
    project_dir: &Path,
    context: InstallContext,
    options: &PythonUpdateOptions<'_>,
) -> Result<bool> {
    let config = context.config;
    if !config.python.enabled {
        return Ok(false);
    }
    let pyproject_path = project_dir.join("pyproject.toml");
    if !pyproject_path.is_file() {
        return Ok(false);
    }
    let declared = declared_dependencies(&pyproject_path)?;
    let targets = select_python_targets(&declared, options)?;
    if options.restrict_to_selectors && targets.is_empty() {
        return Ok(false);
    }
    if !options.restrict_to_selectors && !has_matching_python_group(&declared, options) {
        return Ok(false);
    }
    execute_python_update::<Reporter>(project_dir, context, options, targets).await?;
    Ok(true)
}

pub(crate) fn validate_explicit_python_requests(
    project_dirs: &[PathBuf],
    requests: &[PythonUpdateSelector],
) -> Result<()> {
    let explicit: Vec<&PythonUpdateSelector> = requests
        .iter()
        .filter(|req| req.explicit)
        .collect();
    if explicit.is_empty() {
        return Ok(());
    }
    let mut all_declared = Vec::new();
    for dir in project_dirs {
        let pyproject = dir.join("pyproject.toml");
        if pyproject.is_file() {
            all_declared.extend(declared_dependencies(&pyproject)?);
        }
    }
    for req in explicit {
        let Ok(pkg_name) = PackageName::from_str(&req.name) else {
            bail!("invalid Python package name `{}`", req.name);
        };
        if !all_declared
            .iter()
            .any(|dep| dep.name == pkg_name)
        {
            bail!("package {} is not a declared dependency in pyproject.toml", req.name);
        }
    }
    Ok(())
}

fn has_matching_python_group(
    declared: &[DeclaredPythonDep],
    options: &PythonUpdateOptions<'_>,
) -> bool {
    declared
        .iter()
        .any(|dep| {
            options.step.include_groups.contains(&map_python_group(dep.group))
                && !options.step.ignore_matcher.is_some_and(|matcher| {
                    matcher.matches(dep.name.as_ref())
                })
        })
}

fn select_python_targets(
    declared: &[DeclaredPythonDep],
    options: &PythonUpdateOptions<'_>,
) -> Result<BTreeMap<PackageName, Option<String>>> {
    let mut targets = BTreeMap::new();
    for dep in declared {
        if !options.step.include_groups.contains(&map_python_group(dep.group)) {
            continue;
        }
        let dep_name_str = dep.name.to_string();
        if options.step.ignore_matcher.is_some_and(|matcher| matcher.matches(&dep_name_str)) {
            continue;
        }
        if let Some(req) = options.requests
            .iter()
            .find(|req| {
                PackageName::from_str(&req.name)
                    .ok()
                    .is_some_and(|name| name == dep.name)
            })
        {
            let spec = req.version_spec.as_deref().map(normalize_python_spec);
            targets.insert(dep.name.clone(), spec);
        } else if options.bare_matcher.is_some_and(|matcher| matcher.matches(&dep_name_str)) {
            targets.insert(dep.name.clone(), None);
        }
    }
    Ok(targets)
}

fn normalize_python_spec(spec: &str) -> String {
    if spec.starts_with(['<', '>', '=', '!', '~']) { spec.to_string() } else { format!("=={spec}") }
}

async fn execute_python_update<Reporter: self::Reporter + 'static>(
    project_dir: &Path,
    context: InstallContext,
    options: &PythonUpdateOptions<'_>,
    targets: BTreeMap<PackageName, Option<String>>,
) -> Result<()> {
    let config = context.config;
    let project = pnpm_python_installer::writable_project(project_dir)?;
    let discovery = discover_python_project(config, &project).await?;
    let selected = BTreeSet::from([project.clone()]);
    let update_opts = build_python_update_options(&project, options, targets);
    let task = plan_update::<Reporter>(context.into(), discovery, selected, update_opts)?;
    let root = config.workspace_dir.clone().unwrap_or_else(|| project_dir.to_path_buf());
    InstallPlan::new(root).with_task(task).run().await
}

async fn discover_python_project(
    config: &'static Config,
    project: &Path,
) -> Result<pnpm_python_installer::Discovery> {
    match config.workspace_dir.clone() {
        Some(workspace_root)
            if pnpm_python_installer::in_declared_workspace(&workspace_root, project) =>
        {
            let inventory = EcosystemWorkspaceInventory::new(workspace_root, config);
            eco_python::discover_around(config, &inventory, project).await
        }
        _ => {
            pnpm_python_installer::discover(config, project, &[project.join("pyproject.toml")])
                .await
        }
    }
}

fn build_python_update_options(
    project: &Path,
    options: &PythonUpdateOptions<'_>,
    targets: BTreeMap<PackageName, Option<String>>,
) -> UpdateOptions {
    let restrict_to_targets =
        options.restrict_to_selectors || options.step.ignore_matcher.is_some();
    let targets =
        if !options.restrict_to_selectors && options.step.ignore_matcher.is_some() {
            unignored_python_targets(project, options)
        } else {
            targets
        };
    let groups = [
        (DependencyGroup::Prod, PythonDependencyGroup::Prod),
        (DependencyGroup::Optional, PythonDependencyGroup::Optional),
        (DependencyGroup::Dev, PythonDependencyGroup::Dev),
    ]
    .into_iter()
    .filter_map(|(node_group, py_group)| {
        options.step.include_groups.contains(&node_group).then_some(py_group)
    })
    .collect();
    UpdateOptions {
        targets,
        restrict_to_targets,
        groups,
        latest: options.step.latest,
        no_save: options.step.no_save,
        exact: options.step.save_exact,
        prefix: options.step.save_prefix.map(str::to_string),
    }
}

fn unignored_python_targets(
    project_dir: &Path,
    options: &PythonUpdateOptions<'_>,
) -> BTreeMap<PackageName, Option<String>> {
    let Ok(declared) = declared_dependencies(&project_dir.join("pyproject.toml")) else {
        return BTreeMap::new();
    };
    declared
        .into_iter()
        .filter(|dep| {
            options.step.include_groups.contains(&map_python_group(dep.group))
                && !options.step.ignore_matcher.is_some_and(|matcher| {
                    matcher.matches(dep.name.as_ref())
                })
        })
        .map(|dep| (dep.name, None))
        .collect()
}
