use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pep440_rs::{Operator, Version, VersionSpecifiers};
use pep508_rs::{PackageName, Requirement, VersionOrUrl};
use pnpm_python_resolver::Lockfile;
use rewrite::rewrite_pyproject;

use super::{
    Discovery, InstallOptions, Prepared, Reporter, manifest,
    outdated::{PythonDependencyGroup, locked_versions},
    prepare,
};

mod rewrite;

#[derive(Debug, Clone)]
pub struct UpdateOptions {
    pub targets: BTreeMap<PackageName, Option<String>>,
    pub restrict_to_targets: bool,
    pub groups: BTreeSet<PythonDependencyGroup>,
    pub latest: bool,
    pub no_save: bool,
    pub exact: bool,
    pub prefix: Option<String>,
}

impl UpdateOptions {
    pub fn validate(&self, config: &pnpm_config::Config) -> Result<()> {
        if !config.python.enabled {
            bail!("pypi: dependencies require `python.enabled: true` in pnpm-workspace.yaml");
        }
        if !matches!(self.prefix.as_deref().unwrap_or(">="), ">=" | "~=" | "==") {
            bail!("Python --save-prefix must be >=, ~=, or ==");
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
    projects
        .iter()
        .map(|root| root.join("pyproject.toml"))
        .chain(
            discovery.workspace
                .memberships(selected)
                .iter()
                .map(|membership| membership.root.join("pylock.toml")),
        )
        .collect()
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
    let locked = collect_existing_locks(&memberships);
    if let Err(err) = stage_manifests_for_resolution(&selected, &options, &locked) {
        let _ = restore_manifests(&originals);
        return Err(err);
    }
    let config = context.config;
    let reread = discovery.reread(config, &members).await;
    let discovery = match reread {
        Ok(d) => d,
        Err(err) => {
            let _ = restore_manifests(&originals);
            return Err(err);
        }
    };
    let prepared = prepare::<Reporter>(
        context,
        discovery,
        true,
        manifest::DependencySelection::ALL,
        selected.clone(),
    )
    .await;
    let mut prepared = match prepared {
        Ok(p) => p,
        Err(err) => {
            let _ = restore_manifests(&originals);
            return Err(err);
        }
    };
    finalize_updated(&mut prepared, config, &options, &selected, &originals)?;
    Ok(prepared)
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
) -> BTreeMap<PackageName, Version> {
    let mut map = BTreeMap::new();
    for membership in memberships {
        let lock_path = membership.root.join("pylock.toml");
        if let Ok(versions) = locked_versions(&lock_path) {
            map.extend(versions);
        }
    }
    map
}

fn stage_manifests_for_resolution(
    selected: &BTreeSet<PathBuf>,
    options: &UpdateOptions,
    locked: &BTreeMap<PackageName, Version>,
) -> Result<()> {
    for root in selected {
        let path = root.join("pyproject.toml");
        rewrite_pyproject(&path, |group, req| stage_requirement(group, req, options, locked))?;
    }
    Ok(())
}

fn stage_requirement(
    group: PythonDependencyGroup,
    mut req: Requirement,
    options: &UpdateOptions,
    locked: &BTreeMap<PackageName, Version>,
) -> Result<Option<Requirement>> {
    if matches!(req.version_or_url, Some(VersionOrUrl::Url(_))) {
        return Ok(None);
    }
    if options.selects(group, &req.name) {
        if let Some(Some(explicit_spec)) = options.targets.get(&req.name) {
            req.version_or_url =
                Some(VersionOrUrl::VersionSpecifier(explicit_spec.parse().into_diagnostic()?));
            return Ok(Some(req));
        }
        if options.latest {
            req.version_or_url = None;
            return Ok(Some(req));
        }
        return Ok(None);
    }
    if let Some(locked_ver) = locked.get(&req.name) {
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
    options: &UpdateOptions,
    selected: &BTreeSet<PathBuf>,
    originals: &BTreeMap<PathBuf, String>,
) -> Result<()> {
    restore_manifests(originals)?;
    for project in prepared {
        finalize_prepared_project(project, config, options, selected)?;
    }
    Ok(())
}

fn finalize_prepared_project(
    project: &mut Prepared,
    config: &pnpm_config::Config,
    options: &UpdateOptions,
    selected: &BTreeSet<PathBuf>,
) -> Result<()> {
    let mut lock: Lockfile = toml::from_str(&project.lock).into_diagnostic()?;
    let resolved: BTreeMap<PackageName, Version> = lock.packages
        .iter()
        .filter(|pkg| !pkg.wheels.is_empty() || pkg.sdist.is_some())
        .map(|pkg| (pkg.name.clone(), pkg.version.clone()))
        .collect();
    let mut recorded = Vec::new();
    for member in &project.members {
        let path = member.join("pyproject.toml");
        if !options.no_save && selected.contains(member) {
            rewrite_pyproject(&path, |group, req| {
                finalize_requirement(group, req, options, &resolved)
            })?;
        }
        recorded.extend(read_all_requirements(&path, config)?);
    }
    lock.tool.pnpm.set_requirements(&recorded);
    project.lock = toml::to_string_pretty(&lock).into_diagnostic()?;
    Ok(())
}

fn read_all_requirements(path: &Path, config: &pnpm_config::Config) -> Result<Vec<Requirement>> {
    let contents = fs::read_to_string(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", path.display()))?;
    let manifest = manifest::Manifest::parse(&contents)?;
    manifest.requirements(config, manifest::DependencySelection::ALL)
}

fn finalize_requirement(
    group: PythonDependencyGroup,
    mut req: Requirement,
    options: &UpdateOptions,
    resolved: &BTreeMap<PackageName, Version>,
) -> Result<Option<Requirement>> {
    if !options.selects(group, &req.name)
        || matches!(req.version_or_url, Some(VersionOrUrl::Url(_)))
    {
        return Ok(None);
    }
    let Some(version) = resolved.get(&req.name) else {
        return Ok(None);
    };
    let explicit_spec = options.targets.get(&req.name).and_then(Option::as_deref);
    let new_specifiers = compute_saved_specifiers(&req, version, explicit_spec, options)?;
    let Some(specifiers) = new_specifiers else {
        return Ok(None);
    };
    req.version_or_url = Some(VersionOrUrl::VersionSpecifier(specifiers));
    Ok(Some(req))
}

fn compute_saved_specifiers(
    req: &Requirement,
    version: &Version,
    explicit_spec: Option<&str>,
    options: &UpdateOptions,
) -> Result<Option<VersionSpecifiers>> {
    if options.exact {
        return Ok(Some(format!("=={version}").parse().into_diagnostic()?));
    }
    if let Some(prefix) = options.prefix.as_deref() {
        return Ok(Some(format!("{prefix}{version}").parse().into_diagnostic()?));
    }
    if let Some(spec) = explicit_spec
        && !spec.starts_with("==")
    {
        return Ok(Some(spec.parse().into_diagnostic()?));
    }
    let Some(VersionOrUrl::VersionSpecifier(existing)) = &req.version_or_url else {
        return explicit_spec
            .map(|_| format!(">={version}").parse().into_diagnostic())
            .transpose();
    };
    updated_existing_specifiers(existing, version, options.latest || explicit_spec.is_some())
}

fn updated_existing_specifiers(
    existing: &VersionSpecifiers,
    version: &Version,
    force_bump: bool,
) -> Result<Option<VersionSpecifiers>> {
    if existing.len() == 1 {
        let spec = &existing[0];
        let formatted = match *spec.operator() {
            Operator::Equal => force_bump.then(|| format!("=={version}")),
            Operator::ExactEqual => force_bump.then(|| format!("==={version}")),
            Operator::TildeEqual => Some(format!("~={version}")),
            Operator::GreaterThanEqual => Some(format!(">={version}")),
            _ => (force_bump && !existing.contains(version)).then(|| format!(">={version}")),
        };
        return formatted
            .map(|text| text.parse().into_diagnostic())
            .transpose();
    }
    if force_bump && !existing.contains(version) {
        return Ok(Some(format!(">={version}").parse().into_diagnostic()?));
    }
    let clauses: Vec<String> = existing
        .iter()
        .map(|clause| {
            if *clause.operator() == Operator::GreaterThanEqual {
                format!(">={version}")
            } else {
                clause.to_string()
            }
        })
        .collect();
    Ok(Some(
        clauses
            .join(",")
            .parse()
            .into_diagnostic()?,
    ))
}
