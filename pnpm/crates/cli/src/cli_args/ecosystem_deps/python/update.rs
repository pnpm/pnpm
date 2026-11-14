use super::{PythonUpdateOptions, map_python_group};
use crate::{
    cli_args::ecosystem_deps::selectors::PythonUpdateSelector,
    ecosystem_install::{EcosystemWorkspaceInventory, InstallContext, python as eco_python},
};
use miette::{Result, bail};
use pnpm_config::Config;
use pnpm_install_coordinator::InstallPlan;
use pnpm_package_manifest::DependencyGroup;
use pnpm_python_installer::{
    DeclaredPythonDep, Discovery, PackageName, PythonDependencyGroup, TargetPyproject,
    UpdateOptions, declared_dependencies, in_declared_workspace, inspect_target_pyproject,
    plan_update, writable_project,
};
use pnpm_reporter::Reporter;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    str::FromStr,
};

#[derive(Default)]
pub(crate) struct PythonUpdateTargets {
    expanded: Vec<ExpandedPythonUnit>,
    unexpanded: Vec<UnexpandedPythonProject>,
}

struct ExpandedPythonUnit {
    unit: PythonUpdateUnit,
    declared: Vec<DeclaredPythonDep>,
}

struct UnexpandedPythonProject {
    dir: PathBuf,
    declared: Vec<DeclaredPythonDep>,
}

struct PythonUpdateUnit {
    plan_root: PathBuf,
    discovery: Discovery,
    projects: BTreeSet<PathBuf>,
}

impl PythonUpdateTargets {
    pub(crate) fn declared_names_for_groups(
        &self,
        include: &[DependencyGroup],
    ) -> BTreeSet<String> {
        self.all_declared()
            .filter(|dep| include.contains(&map_python_group(dep.group)))
            .map(|dep| dep.name.to_string())
            .collect()
    }

    fn all_declared(&self) -> impl Iterator<Item = &DeclaredPythonDep> {
        self.expanded
            .iter()
            .flat_map(|item| &item.declared)
            .chain(self.unexpanded.iter().flat_map(|item| &item.declared))
    }
}

pub(crate) async fn resolve_python_update_targets(
    target_dirs: &[PathBuf],
    config: &'static Config,
) -> Result<PythonUpdateTargets> {
    if !config.python.enabled {
        return Ok(PythonUpdateTargets::default());
    }
    let (shared_groups, mut unexpanded) = classify_python_target_dirs(target_dirs, config)?;
    let mut expanded = Vec::new();
    let mut covered_dirs = BTreeSet::new();
    for (workspace_root, dirs) in shared_groups {
        if let Some(unit) = discover_workspace_unit(workspace_root, &dirs, config).await? {
            for proj in &unit.projects {
                covered_dirs.insert(canonical_or_self(proj));
            }
            let declared = collect_declared_dependencies(&unit.projects, Some(&unit.plan_root))?;
            expanded.push(ExpandedPythonUnit { unit, declared });
        }
    }
    if !covered_dirs.is_empty() {
        unexpanded.retain(|proj| !covered_dirs.contains(&canonical_or_self(&proj.dir)));
    }
    Ok(PythonUpdateTargets { expanded, unexpanded })
}

type WorkspaceDirGroups = BTreeMap<PathBuf, Vec<PathBuf>>;

fn classify_python_target_dirs(
    target_dirs: &[PathBuf],
    config: &Config,
) -> Result<(WorkspaceDirGroups, Vec<UnexpandedPythonProject>)> {
    let mut shared_groups: WorkspaceDirGroups = BTreeMap::new();
    let mut unexpanded = Vec::new();
    let workspace_root = config.workspace_dir.as_deref();
    for dir in target_dirs {
        match inspect_target_pyproject(&dir.join("pyproject.toml"), workspace_root)? {
            Some(TargetPyproject::SharedWorkspace) => {
                let root = config.workspace_dir.clone().unwrap_or_else(|| dir.clone());
                shared_groups
                    .entry(root)
                    .or_default()
                    .push(dir.clone());
            }
            Some(TargetPyproject::Project(declared)) => {
                unexpanded.push(UnexpandedPythonProject { dir: dir.clone(), declared });
            }
            None => {}
        }
    }
    Ok((shared_groups, unexpanded))
}

fn canonical_or_self(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn partition_matching_dirs(
    dirs: Vec<PathBuf>,
    config: &Config,
) -> (WorkspaceDirGroups, Vec<PathBuf>) {
    let mut workspace_groups: WorkspaceDirGroups = BTreeMap::new();
    let mut standalone_dirs = Vec::new();
    for dir in dirs {
        let workspace_root = config.workspace_dir.clone().unwrap_or_else(|| dir.clone());
        if in_declared_workspace(&workspace_root, &dir) {
            workspace_groups
                .entry(workspace_root)
                .or_default()
                .push(dir);
        } else {
            standalone_dirs.push(dir);
        }
    }
    (workspace_groups, standalone_dirs)
}

async fn discover_workspace_unit(
    workspace_root: PathBuf,
    dirs: &[PathBuf],
    config: &'static Config,
) -> Result<Option<PythonUpdateUnit>> {
    let inventory = EcosystemWorkspaceInventory::new(workspace_root.clone(), config);
    let discovery = eco_python::discover_around_projects(config, &inventory, dirs).await?;
    let projects: BTreeSet<PathBuf> = dirs
        .iter()
        .flat_map(|dir| discovery.target_projects(dir))
        .collect();
    if projects.is_empty() {
        return Ok(None);
    }
    Ok(Some(PythonUpdateUnit { plan_root: workspace_root, discovery, projects }))
}

async fn discover_standalone_unit(
    dir: &Path,
    config: &'static Config,
) -> Result<Option<PythonUpdateUnit>> {
    let project = writable_project(dir)?;
    let discovery =
        pnpm_python_installer::discover(config, &project, &[project.join("pyproject.toml")]).await?;
    let projects: BTreeSet<PathBuf> = discovery
        .target_projects(&project)
        .into_iter()
        .collect();
    if projects.is_empty() {
        return Ok(None);
    }
    let plan_root = config.workspace_dir.clone().unwrap_or(project);
    Ok(Some(PythonUpdateUnit { plan_root, discovery, projects }))
}

pub(crate) fn validate_explicit_python_requests(
    targets: &PythonUpdateTargets,
    requests: &[PythonUpdateSelector],
) -> Result<()> {
    let explicit: Vec<&PythonUpdateSelector> = requests
        .iter()
        .filter(|req| req.explicit)
        .collect();
    if explicit.is_empty() {
        return Ok(());
    }
    let all_declared: Vec<&DeclaredPythonDep> = targets.all_declared().collect();
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

pub(crate) async fn update_python_projects<ReporterType: Reporter + 'static>(
    targets: PythonUpdateTargets,
    context: InstallContext,
    options: &PythonUpdateOptions<'_>,
) -> Result<bool> {
    if !context.config.python.enabled {
        return Ok(false);
    }
    let mut updated = false;
    for expanded in targets.expanded {
        updated |= run_python_unit::<ReporterType>(
            expanded.unit,
            &expanded.declared,
            context.clone(),
            options,
        )
        .await?;
    }
    let matching_dirs: Vec<PathBuf> = targets.unexpanded
        .into_iter()
        .filter_map(|proj| {
            declared_matches_options(&proj.declared, options)
                .map(|has_match| has_match.then_some(proj.dir))
                .transpose()
        })
        .collect::<Result<Vec<_>>>()?;
    if matching_dirs.is_empty() {
        return Ok(updated);
    }
    updated |= update_matching_unexpanded::<ReporterType>(matching_dirs, context, options).await?;
    Ok(updated)
}

async fn update_matching_unexpanded<ReporterType: Reporter + 'static>(
    matching_dirs: Vec<PathBuf>,
    context: InstallContext,
    options: &PythonUpdateOptions<'_>,
) -> Result<bool> {
    let (workspace_groups, standalone_dirs) =
        partition_matching_dirs(matching_dirs, context.config);
    let mut updated = false;
    for (workspace_root, dirs) in workspace_groups {
        if let Some(unit) = discover_workspace_unit(workspace_root, &dirs, context.config).await? {
            let declared = collect_declared_dependencies(&unit.projects, Some(&unit.plan_root))?;
            updated |=
                run_python_unit::<ReporterType>(unit, &declared, context.clone(), options).await?;
        }
    }
    for dir in standalone_dirs {
        if let Some(unit) = discover_standalone_unit(&dir, context.config).await? {
            let declared = collect_declared_dependencies(
                &unit.projects,
                context.config.workspace_dir.as_deref(),
            )?;
            updated |=
                run_python_unit::<ReporterType>(unit, &declared, context.clone(), options).await?;
        }
    }
    Ok(updated)
}

fn declared_matches_options(
    declared: &[DeclaredPythonDep],
    options: &PythonUpdateOptions<'_>,
) -> Result<bool> {
    if options.restrict_to_selectors {
        Ok(!select_python_targets(declared, options)?.is_empty())
    } else {
        Ok(has_matching_python_group(declared, options))
    }
}

async fn run_python_unit<ReporterType: Reporter + 'static>(
    unit: PythonUpdateUnit,
    declared: &[DeclaredPythonDep],
    context: InstallContext,
    options: &PythonUpdateOptions<'_>,
) -> Result<bool> {
    let targets = select_python_targets(declared, options)?;
    if options.restrict_to_selectors && targets.is_empty() {
        return Ok(false);
    }
    if !options.restrict_to_selectors && !has_matching_python_group(declared, options) {
        return Ok(false);
    }
    let update_opts = build_python_update_options(declared, options, targets);
    let task =
        plan_update::<ReporterType>(context.into(), unit.discovery, unit.projects, update_opts)?;
    InstallPlan::new(unit.plan_root).with_task(task).run().await?;
    Ok(true)
}

fn collect_declared_dependencies(
    projects: &BTreeSet<PathBuf>,
    workspace_root: Option<&Path>,
) -> Result<Vec<DeclaredPythonDep>> {
    let mut declared = Vec::new();
    for dir in projects {
        let pyproject = dir.join("pyproject.toml");
        if pyproject.is_file() {
            declared.extend(declared_dependencies(&pyproject, workspace_root)?);
        }
    }
    Ok(declared)
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
        if is_python_dep_excluded(&dep_name_str, options) {
            continue;
        }
        if let Some(req) = find_python_request(&dep.name, options.requests) {
            let spec = req.version_spec.as_deref().map(normalize_python_spec);
            targets.insert(dep.name.clone(), spec);
            continue;
        }
        if options.bare_matcher.is_some_and(|matcher| matcher.matches(&dep_name_str))
            || options.update_all_except_exclusions
        {
            targets.insert(dep.name.clone(), None);
        }
    }
    Ok(targets)
}

fn is_python_dep_excluded(name: &str, options: &PythonUpdateOptions<'_>) -> bool {
    options.step.ignore_matcher.is_some_and(|matcher| matcher.matches(name))
        || options.exclusions
            .iter()
            .any(|excluded| excluded == name)
}

fn find_python_request<'a>(
    name: &PackageName,
    requests: &'a [PythonUpdateSelector],
) -> Option<&'a PythonUpdateSelector> {
    requests
        .iter()
        .find(|req| {
            PackageName::from_str(&req.name).is_ok_and(|request_name| request_name == *name)
        })
}

fn normalize_python_spec(spec: &str) -> String {
    if spec.starts_with(['<', '>', '=', '!', '~']) { spec.to_string() } else { format!("=={spec}") }
}

fn build_python_update_options(
    declared: &[DeclaredPythonDep],
    options: &PythonUpdateOptions<'_>,
    targets: BTreeMap<PackageName, Option<String>>,
) -> UpdateOptions {
    let restrict_to_targets =
        options.restrict_to_selectors || options.step.ignore_matcher.is_some();
    let targets =
        if !options.restrict_to_selectors && options.step.ignore_matcher.is_some() {
            unignored_python_targets(declared, options)
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
    }
}

fn unignored_python_targets(
    declared: &[DeclaredPythonDep],
    options: &PythonUpdateOptions<'_>,
) -> BTreeMap<PackageName, Option<String>> {
    declared
        .iter()
        .filter(|dep| {
            options.step.include_groups.contains(&map_python_group(dep.group))
                && !options.step.ignore_matcher.is_some_and(|matcher| {
                    matcher.matches(dep.name.as_ref())
                })
        })
        .map(|dep| (dep.name.clone(), None))
        .collect()
}
