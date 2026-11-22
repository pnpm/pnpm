use super::{
    Discovery,
    InstallOptions,
    Lockfile,
    Prepared,
    Reporter,
    manifest,
    prepare,
    workspace::Workspace,
};
use miette::{
    IntoDiagnostic,
    Result,
    WrapErr,
    bail,
};
use pnpm_python_resolver::LockIndex;
use std::{
    collections::BTreeSet,
    path::{
        Path,
        PathBuf,
    },
};

#[derive(Debug, Clone)]
pub struct AddOptions {
    pub requirements: Vec<String>,
    pub development: bool,
    pub exact: bool,
    pub prefix: Option<String>,
}

impl AddOptions {
    /// Refuse an add pnpm cannot carry out. Call it before reading a
    /// manifest, so an add pnpm will not carry out leaves the workspace as
    /// it found it.
    pub fn validate(&self, config: &pnpm_config::Config) -> Result<()> {
        if !config.python.enabled {
            bail!("pypi: dependencies require `python.enabled: true` in pnpm-workspace.yaml");
        }
        if !matches!(self.prefix.as_deref().unwrap_or(">="), ">=" | "~=" | "==") {
            bail!("Python --save-prefix must be >=, ~=, or ==");
        }
        Ok(())
    }
}

/// Add requirements to each selected Python project, then prepare those
/// projects as one participant in an install plan.
pub fn plan_add<Reporter: self::Reporter + 'static>(
    context: InstallOptions,
    discovery: Discovery,
    selected: BTreeSet<PathBuf>,
    options: AddOptions,
) -> Result<pnpm_install_coordinator::InstallTask<'static>> {
    options.validate(context.config)?;
    for project in &selected {
        writable_project(project)?;
    }
    Ok(plan_edit::<Reporter>(context, discovery, selected, ManifestEdit::Add(options)))
}

enum ManifestEdit {
    Add(AddOptions),
    Remove(BTreeSet<pep508_rs::PackageName>, Option<crate::PythonDependencyGroup>),
}

impl ManifestEdit {
    fn apply(&self, selected: &BTreeSet<PathBuf>, config: &pnpm_config::Config) -> Result<()> {
        let mut removed = BTreeSet::new();
        for root in selected {
            let path = root.join("pyproject.toml");
            match self {
                ManifestEdit::Add(options) => {
                    manifest::add(&path, &options.requirements, options.development, config)?;
                }
                ManifestEdit::Remove(names, group) => {
                    crate::update::rewrite::remove_requirements(
                        &path,
                        names,
                        *group,
                        &mut removed,
                    )?;
                }
            }
        }
        if let ManifestEdit::Remove(names, _) = self
            && let Some(name) = names.difference(&removed).next()
        {
            bail!("cannot remove {name}: not declared in the selected Python dependencies");
        }
        Ok(())
    }
}

/// Remove matching requirements across the selected projects and prepare their environments.
pub fn plan_remove<Reporter: self::Reporter + 'static>(
    context: InstallOptions,
    discovery: Discovery,
    selected: BTreeSet<PathBuf>,
    names: BTreeSet<pep508_rs::PackageName>,
    group: Option<crate::PythonDependencyGroup>,
) -> pnpm_install_coordinator::InstallTask<'static> {
    plan_edit::<Reporter>(context, discovery, selected, ManifestEdit::Remove(names, group))
}

fn plan_edit<Reporter: self::Reporter + 'static>(
    context: InstallOptions,
    discovery: Discovery,
    selected: BTreeSet<PathBuf>,
    edit: ManifestEdit,
) -> pnpm_install_coordinator::InstallTask<'static> {
    let metadata = metadata_paths(&discovery, &selected);
    let prepare = async move {
        let config = context.config;
        edit.apply(&selected, config)?;
        // Every member's manifest is read again, not only the edited ones:
        // the discovery above ran before the workspace lock was taken.
        let members = discovery.workspace
            .memberships(&selected)
            .into_iter()
            .flat_map(|membership| membership.members)
            .collect();
        let discovery = discovery.reread(config, &members).await?;
        let workspace = discovery.workspace.clone();
        let mut prepared = prepare::<Reporter>(
            context,
            discovery,
            true,
            manifest::DependencySelection::ALL,
            selected.clone(),
        )
        .await?;
        if let ManifestEdit::Add(options) = edit {
            save_added(&mut prepared, config, &workspace, &options, &selected).await?;
        }
        Ok(prepared)
    };
    pnpm_install_coordinator::InstallTask::new(metadata, prepare)
}

/// The files an add may change: each edited manifest, and the lockfile of
/// every unit the edited projects install into.
fn metadata_paths(discovery: &Discovery, selected: &BTreeSet<PathBuf>) -> Vec<PathBuf> {
    selected
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

/// A directory `pnpm add` can write a requirement to. A directory without a
/// manifest is named here; one whose manifest declares no project is named
/// by the edit itself.
///
/// Call it before reading the manifest, so a directory that has none is
/// named rather than reported as a file that could not be read.
pub fn writable_project(root: &Path) -> Result<PathBuf> {
    let path = root.join("pyproject.toml");
    if !path
        .try_exists()
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", path.display()))?
    {
        let missing = path.display();
        return Err(miette::miette!(
            help = "Run the command in a directory that has a pyproject.toml, or create one there.",
            "cannot add a Python dependency because {missing} does not exist",
        ));
    }
    Ok(root.to_path_buf())
}

/// Write the versions the lockfile resolved back to the manifests the add
/// edited, and record what the members require now in the lockfile.
async fn save_added(
    prepared: &mut [Prepared],
    config: &pnpm_config::Config,
    workspace: &Workspace,
    options: &AddOptions,
    edited: &BTreeSet<PathBuf>,
) -> Result<()> {
    let prefix = options.prefix.as_deref().unwrap_or(">=");
    for project in prepared {
        let mut lock: Lockfile = toml::from_str(&project.lock).into_diagnostic()?;
        let lock_index = lock.index()?;
        let requirements = pinned_requirements(options, &lock_index, prefix)?;
        for member in &project.members {
            if edited.contains(member) {
                manifest::add(
                    &member.join("pyproject.toml"),
                    &requirements,
                    options.development,
                    config,
                )?;
            }
        }
        let recorded = workspace.recorded_requirements(config, &project.members).await?;
        lock.tool.pnpm.set_requirements(&recorded);
        project.lock = toml::to_string_pretty(&lock).into_diagnostic()?;
    }
    Ok(())
}

fn pinned_requirements(
    options: &AddOptions,
    lock_index: &LockIndex<'_>,
    prefix: &str,
) -> Result<Vec<String>> {
    options.requirements
        .iter()
        .map(|text| {
            let mut requirement = pnpm_python_resolver::parse_requirement(text)?;
            pin_to_locked_version(&mut requirement, lock_index, options, prefix)?;
            Ok(requirement.to_string())
        })
        .collect()
}

/// Give a requirement the version the lockfile resolved, when the command
/// line left it unversioned or `--save-exact` overrides what it asked for.
fn pin_to_locked_version(
    requirement: &mut pep508_rs::Requirement,
    lock_index: &LockIndex<'_>,
    options: &AddOptions,
    prefix: &str,
) -> Result<()> {
    if matches!(requirement.version_or_url, Some(pep508_rs::VersionOrUrl::Url(_))) {
        return Ok(());
    }
    if !options.exact && requirement.version_or_url.is_some() {
        return Ok(());
    }
    let Some(version) = lock_index.agreed_version(requirement) else {
        return Ok(());
    };
    let prefix = if options.exact { "==" } else { prefix };
    requirement.version_or_url = Some(pep508_rs::VersionOrUrl::VersionSpecifier(
        format!("{prefix}{version}").parse().into_diagnostic()?,
    ));
    Ok(())
}

#[cfg(test)]
mod tests;
