use super::{
    Discovery, InstallOptions, Prepared, Reporter, manifest,
    outdated::{PythonDependencyGroup, declared_dependencies_in_workspace},
    prepare,
    workspace::Workspace,
};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pep508_rs::{MarkerTree, PackageName, Requirement, VersionOrUrl};
use pnpm_python_resolver::{LockIndex, Lockfile};
use rewrite::{rewrite_pyproject, rewrite_pyproject_many};
use specifiers::compute_saved_specifiers_for_pins;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

mod rewrite;
mod specifiers;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
pub struct UpdateOptions {
    pub targets: BTreeMap<PackageName, Option<String>>,
    pub restrict_to_targets: bool,
    pub groups: BTreeSet<PythonDependencyGroup>,
    pub latest: bool,
    pub no_save: bool,
}

impl UpdateOptions {
    pub fn validate(&self, config: &pnpm_config::Config) -> Result<()> {
        if !config.python.enabled {
            bail!("pypi: dependencies require `python.enabled: true` in pnpm-workspace.yaml");
        }
        Ok(())
    }

    fn selects(&self, group: PythonDependencyGroup, name: &PackageName) -> bool {
        self.groups.contains(&group)
            && (!self.restrict_to_targets || self.targets.contains_key(name))
    }
}

/// Update requirements in each selected Python project and re-resolve their
/// lockfiles and environments.
pub fn plan_update<Reporter: self::Reporter + 'static>(
    context: InstallOptions,
    discovery: Discovery,
    selected: BTreeSet<PathBuf>,
    options: UpdateOptions,
) -> Result<pnpm_install_coordinator::InstallTask<'static>> {
    options.validate(context.config)?;
    let projects = selected
        .iter()
        .map(|root| super::add::writable_project(root))
        .collect::<Result<Vec<_>>>()?;
    let metadata = update_metadata_paths(&discovery, &projects, &selected);
    let prepare_future =
        async move { run_update_prepare::<Reporter>(context, discovery, selected, options).await };
    Ok(pnpm_install_coordinator::InstallTask::new(metadata, prepare_future))
}

fn update_metadata_paths(
    discovery: &Discovery,
    projects: &[PathBuf],
    selected: &BTreeSet<PathBuf>,
) -> Vec<PathBuf> {
    let memberships = discovery.workspace.memberships(selected);
    projects
        .iter()
        .chain(memberships.iter().flat_map(|membership| &membership.members))
        .map(|root| root.join("pyproject.toml"))
        .chain(
            memberships
                .iter()
                .map(|membership| membership.root.join("pylock.toml")),
        )
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) type UpdatedMarkersByLockRoot = BTreeMap<PathBuf, BTreeMap<PackageName, MarkerTree>>;

pub(super) struct StageContext<'a> {
    pub(super) discovery: &'a Discovery,
    pub(super) selected: &'a BTreeSet<PathBuf>,
    pub(super) updated_markers: UpdatedMarkersByLockRoot,
    pub(super) options: &'a UpdateOptions,
    pub(super) locks: BTreeMap<PathBuf, Lockfile>,
}

async fn run_update_prepare<Reporter: self::Reporter + 'static>(
    context: InstallOptions,
    discovery: Discovery,
    selected: BTreeSet<PathBuf>,
    options: UpdateOptions,
) -> Result<Vec<Prepared>> {
    let memberships = discovery.workspace.memberships(&selected);
    let members: BTreeSet<PathBuf> = memberships
        .iter()
        .flat_map(|membership| membership.members.iter().cloned())
        .collect();
    let originals = snapshot_manifests(&members)?;
    let config = context.config;
    let workspace = discovery.workspace.clone();
    let mut prepared = stage_and_prepare::<Reporter>(
        context,
        discovery,
        &memberships,
        &members,
        &selected,
        &options,
    )
    .await
    .inspect_err(|_| {
        let _ = restore_manifests(&originals);
    })?;
    finalize_updated(&mut prepared, config, &workspace, &options, &selected, &originals)?;
    Ok(prepared)
}

async fn stage_and_prepare<Reporter: self::Reporter + 'static>(
    context: InstallOptions,
    discovery: Discovery,
    memberships: &[super::workspace::members::Membership],
    members: &BTreeSet<PathBuf>,
    selected: &BTreeSet<PathBuf>,
    options: &UpdateOptions,
) -> Result<Vec<Prepared>> {
    let stage = StageContext {
        discovery: &discovery,
        selected,
        updated_markers: collect_updated_markers(&discovery, selected, options)?,
        options,
        locks: collect_existing_locks(memberships),
    };
    stage_manifests_for_resolution(members, &stage)?;
    let discovery = discovery.reread(context.config, members).await?;
    prepare::<Reporter>(
        context,
        discovery,
        true,
        manifest::DependencySelection::ALL,
        selected.clone(),
    )
    .await
}

pub(super) fn collect_updated_markers(
    discovery: &Discovery,
    selected: &BTreeSet<PathBuf>,
    options: &UpdateOptions,
) -> Result<UpdatedMarkersByLockRoot> {
    let mut by_lock_root: UpdatedMarkersByLockRoot = BTreeMap::new();
    for root in selected {
        let lock_root = discovery.workspace.lock_root(root);
        let declared =
            declared_dependencies_in_workspace(&root.join("pyproject.toml"), &discovery.workspace)?;
        let entry = by_lock_root.entry(lock_root.to_path_buf()).or_default();
        record_selected_markers(entry, declared, options);
    }
    Ok(by_lock_root)
}

fn record_selected_markers(
    markers: &mut BTreeMap<PackageName, MarkerTree>,
    declared: Vec<super::DeclaredPythonDep>,
    options: &UpdateOptions,
) {
    for dep in declared {
        if !options.selects(dep.group, &dep.name) {
            continue;
        }
        let branch = dep.requirement.marker.simplify_extras_with(|_| true);
        markers
            .entry(dep.name)
            .and_modify(|existing| existing.or(branch.clone()))
            .or_insert(branch);
    }
}

fn snapshot_manifests(members: &BTreeSet<PathBuf>) -> Result<BTreeMap<PathBuf, String>> {
    let mut originals = BTreeMap::new();
    for member in members {
        let path = member.join("pyproject.toml");
        if path.is_file() {
            let contents = fs::read_to_string(&path)
                .into_diagnostic()
                .wrap_err_with(|| format!("read {}", path.display()))?;
            originals.insert(path, contents);
        }
    }
    Ok(originals)
}

fn restore_manifests(originals: &BTreeMap<PathBuf, String>) -> Result<()> {
    for (path, contents) in originals {
        pnpm_fs::write_atomic(path, contents.as_bytes()).into_diagnostic()?;
    }
    Ok(())
}

fn collect_existing_locks(
    memberships: &[super::workspace::members::Membership],
) -> BTreeMap<PathBuf, Lockfile> {
    let mut locks = BTreeMap::new();
    for membership in memberships {
        let lock_path = membership.root.join("pylock.toml");
        if let Ok(contents) = fs::read_to_string(&lock_path)
            && let Ok(lock) = toml::from_str::<Lockfile>(&contents)
        {
            locks.insert(membership.root.clone(), lock);
        }
    }
    locks
}

pub(super) fn stage_manifests_for_resolution(
    members: &BTreeSet<PathBuf>,
    stage: &StageContext<'_>,
) -> Result<()> {
    let lock_indices: BTreeMap<&Path, LockIndex<'_>> = stage.locks
        .iter()
        .map(|(root, lock)| Ok((root.as_path(), lock.index()?)))
        .collect::<Result<_>>()?;
    for member in members {
        let is_selected = stage.selected.contains(member);
        let lock_root = stage.discovery.workspace.lock_root(member);
        let lock_index = lock_indices.get(lock_root);
        let updated_in_env = stage.updated_markers.get(lock_root);
        rewrite_pyproject_many(member, &stage.discovery.workspace, |group, req| {
            stage_requirement(group, req, is_selected, stage.options, updated_in_env, lock_index)
        })?;
    }
    Ok(())
}

pub(super) fn stage_requirement(
    group: PythonDependencyGroup,
    mut req: Requirement,
    is_selected: bool,
    options: &UpdateOptions,
    updated_in_env: Option<&BTreeMap<PackageName, MarkerTree>>,
    lock_index: Option<&LockIndex<'_>>,
) -> Result<Option<Vec<Requirement>>> {
    if matches!(req.version_or_url, Some(VersionOrUrl::Url(_))) {
        return Ok(None);
    }
    if is_selected && options.selects(group, &req.name) {
        return stage_selected_requirement(&mut req, options);
    }
    let Some(lock_index) = lock_index else {
        return Ok(None);
    };
    let excluded = updated_in_env.and_then(|markers| markers.get(&req.name));
    lock_index.pinned_index_requirements(&req, excluded)
}

fn stage_selected_requirement(
    req: &mut Requirement,
    options: &UpdateOptions,
) -> Result<Option<Vec<Requirement>>> {
    if let Some(Some(explicit_spec)) = options.targets.get(&req.name) {
        req.version_or_url =
            Some(VersionOrUrl::VersionSpecifier(explicit_spec.parse().into_diagnostic()?));
        return Ok(Some(vec![req.clone()]));
    }
    if options.latest {
        req.version_or_url = None;
        return Ok(Some(vec![req.clone()]));
    }
    Ok(None)
}

fn finalize_updated(
    prepared: &mut [Prepared],
    config: &pnpm_config::Config,
    workspace: &Workspace,
    options: &UpdateOptions,
    selected: &BTreeSet<PathBuf>,
    originals: &BTreeMap<PathBuf, String>,
) -> Result<()> {
    restore_manifests(originals)?;
    for project in prepared {
        finalize_prepared_project(project, config, workspace, options, selected)?;
    }
    Ok(())
}

fn finalize_prepared_project(
    project: &mut Prepared,
    config: &pnpm_config::Config,
    workspace: &Workspace,
    options: &UpdateOptions,
    selected: &BTreeSet<PathBuf>,
) -> Result<()> {
    let mut lock: Lockfile = toml::from_str(&project.lock).into_diagnostic()?;
    let lock_index = lock.index()?;
    for member in project.members
        .iter()
        .filter(|member| !options.no_save && selected.contains(*member))
    {
        rewrite_pyproject(member, workspace, |group, req| {
            finalize_requirement(group, req, options, &lock_index)
        })?;
    }
    let recorded = workspace.recorded_requirements(config, &project.members)?;
    lock.tool.pnpm.set_requirements(&recorded);
    project.lock = toml::to_string_pretty(&lock).into_diagnostic()?;
    Ok(())
}

pub(super) fn finalize_requirement(
    group: PythonDependencyGroup,
    mut req: Requirement,
    options: &UpdateOptions,
    lock_index: &LockIndex<'_>,
) -> Result<Option<Requirement>> {
    if !options.selects(group, &req.name)
        || matches!(req.version_or_url, Some(VersionOrUrl::Url(_)))
    {
        return Ok(None);
    }
    let explicit_spec = options.targets.get(&req.name).and_then(Option::as_deref);
    if let Some(spec) = explicit_spec.filter(|spec| !spec.starts_with("==")) {
        req.version_or_url = Some(VersionOrUrl::VersionSpecifier(spec.parse().into_diagnostic()?));
        return Ok(Some(req));
    }
    let pins = lock_index.matching_index_pins(&req);
    let Some(specifiers) =
        compute_saved_specifiers_for_pins(&req, &pins, explicit_spec, options.latest)?
    else {
        return Ok(None);
    };
    req.version_or_url = Some(VersionOrUrl::VersionSpecifier(specifiers));
    Ok(Some(req))
}
