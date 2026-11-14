use super::{
    Discovery, InstallOptions, Prepared, Reporter, manifest,
    outdated::{PythonDependencyGroup, declared_dependencies},
    prepare,
    workspace::Workspace,
};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pep508_rs::{PackageName, Requirement, VersionOrUrl};
use pnpm_python_resolver::Lockfile;
use rewrite::rewrite_pyproject;
use specifiers::compute_saved_specifiers;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
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

struct StageContext<'a> {
    discovery: &'a Discovery,
    selected: &'a BTreeSet<PathBuf>,
    updated_names: BTreeSet<PackageName>,
    options: &'a UpdateOptions,
    locks: BTreeMap<PathBuf, Lockfile>,
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
        updated_names: collect_updated_names(selected, options)?,
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

fn collect_updated_names(
    selected: &BTreeSet<PathBuf>,
    options: &UpdateOptions,
) -> Result<BTreeSet<PackageName>> {
    let mut names = BTreeSet::new();
    for root in selected {
        for dep in declared_dependencies(&root.join("pyproject.toml"))? {
            if options.selects(dep.group, &dep.name) {
                names.insert(dep.name);
            }
        }
    }
    Ok(names)
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

fn stage_manifests_for_resolution(
    members: &BTreeSet<PathBuf>,
    stage: &StageContext<'_>,
) -> Result<()> {
    for member in members {
        let is_selected = stage.selected.contains(member);
        let lock = stage.locks.get(stage.discovery.workspace.lock_root(member));
        let path = member.join("pyproject.toml");
        rewrite_pyproject(&path, |group, req| {
            stage_requirement(group, req, is_selected, stage, lock)
        })?;
    }
    Ok(())
}

fn stage_requirement(
    group: PythonDependencyGroup,
    mut req: Requirement,
    is_selected: bool,
    stage: &StageContext<'_>,
    lock: Option<&Lockfile>,
) -> Result<Option<Requirement>> {
    if matches!(req.version_or_url, Some(VersionOrUrl::Url(_))) {
        return Ok(None);
    }
    if is_selected && stage.options.selects(group, &req.name) {
        if let Some(Some(explicit_spec)) = stage.options.targets.get(&req.name) {
            req.version_or_url =
                Some(VersionOrUrl::VersionSpecifier(explicit_spec.parse().into_diagnostic()?));
            return Ok(Some(req));
        }
        if stage.options.latest {
            req.version_or_url = None;
            return Ok(Some(req));
        }
        return Ok(None);
    }
    if !is_selected && stage.updated_names.contains(&req.name) {
        return Ok(None);
    }
    if let Some(locked_ver) = lock
        .and_then(|lock| lock.resolved_version(&req, true).ok())
        .flatten()
    {
        req.version_or_url = Some(VersionOrUrl::VersionSpecifier(
            format!("=={locked_ver}").parse().into_diagnostic()?,
        ));
        return Ok(Some(req));
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
    for member in project.members
        .iter()
        .filter(|member| !options.no_save && selected.contains(*member))
    {
        rewrite_pyproject(&member.join("pyproject.toml"), |group, req| {
            finalize_requirement(group, req, options, &lock)
        })?;
    }
    let recorded = workspace.recorded_requirements(config, &project.members)?;
    lock.tool.pnpm.set_requirements(&recorded);
    project.lock = toml::to_string_pretty(&lock).into_diagnostic()?;
    Ok(())
}

fn finalize_requirement(
    group: PythonDependencyGroup,
    mut req: Requirement,
    options: &UpdateOptions,
    lock: &Lockfile,
) -> Result<Option<Requirement>> {
    if !options.selects(group, &req.name)
        || matches!(req.version_or_url, Some(VersionOrUrl::Url(_)))
    {
        return Ok(None);
    }
    let Some(version) = lock.resolved_version(&req, true)? else {
        return Ok(None);
    };
    let explicit_spec = options.targets.get(&req.name).and_then(Option::as_deref);
    let Some(specifiers) = compute_saved_specifiers(&req, version, explicit_spec, options.latest)?
    else {
        return Ok(None);
    };
    req.version_or_url = Some(VersionOrUrl::VersionSpecifier(specifiers));
    Ok(Some(req))
}
