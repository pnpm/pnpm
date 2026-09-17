use super::{Discovery, InstallOptions, Lockfile, Prepared, Reporter, manifest, prepare};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
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
    let projects = selected
        .iter()
        .map(|root| writable_project(root))
        .collect::<Result<Vec<_>>>()?;
    let metadata = projects
        .iter()
        .map(|root| root.join("pyproject.toml"))
        .chain(
            discovery.workspace
                .memberships(&selected)
                .iter()
                .map(|membership| membership.root.join("pylock.toml")),
        )
        .collect();
    let prepare = async move {
        for root in &projects {
            manifest::add(
                &root.join("pyproject.toml"),
                &options.requirements,
                options.development,
            )?;
        }
        let config = context.config;
        let discovery = discovery.reread(config, &selected).await?;
        let mut prepared = prepare::<Reporter>(
            context,
            discovery,
            true,
            manifest::DependencySelection::ALL,
            selected.clone(),
        )
        .await?;
        save_added(&mut prepared, config, &options, &selected)?;
        Ok(prepared)
    };
    Ok(pnpm_install_coordinator::InstallTask::new(metadata, prepare))
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
fn save_added(
    prepared: &mut [Prepared],
    config: &pnpm_config::Config,
    options: &AddOptions,
    edited: &BTreeSet<PathBuf>,
) -> Result<()> {
    let prefix = options.prefix.as_deref().unwrap_or(">=");
    for project in prepared {
        let mut lock: Lockfile = toml::from_str(&project.lock).into_diagnostic()?;
        let mut requirements = Vec::new();
        for requirement in &options.requirements {
            let mut requirement = pnpm_python_resolver::parse_requirement(requirement)?;
            pin_to_locked_version(&mut requirement, &lock, options, prefix)?;
            requirements.push(requirement.to_string());
        }
        let mut recorded = Vec::new();
        for member in &project.members {
            let path = member.join("pyproject.toml");
            if edited.contains(member) {
                manifest::add(&path, &requirements, options.development)?;
            }
            recorded.extend(read_requirements(&path, config)?);
        }
        lock.tool.pnpm.set_requirements(&recorded);
        project.lock = toml::to_string_pretty(&lock).into_diagnostic()?;
    }
    Ok(())
}

fn read_requirements(
    path: &Path,
    config: &pnpm_config::Config,
) -> Result<Vec<pep508_rs::Requirement>> {
    let manifest = manifest::Manifest::parse(
        &fs::read_to_string(path)
            .into_diagnostic()
            .wrap_err_with(|| format!("read {}", path.display()))?,
    )?;
    manifest.requirements(config, manifest::DependencySelection::ALL)
}

/// Give a requirement the version the lockfile resolved, when the command
/// line left it unversioned or `--save-exact` overrides what it asked for.
fn pin_to_locked_version(
    requirement: &mut pep508_rs::Requirement,
    lock: &Lockfile,
    options: &AddOptions,
    prefix: &str,
) -> Result<()> {
    if !options.exact && requirement.version_or_url.is_some() {
        return Ok(());
    }
    let Some(package) = lock.packages
        .iter()
        .find(|package| package.name == requirement.name)
    else {
        return Ok(());
    };
    let prefix = if options.exact { "==" } else { prefix };
    requirement.version_or_url = Some(pep508_rs::VersionOrUrl::VersionSpecifier(
        format!("{prefix}{}", package.version).parse().into_diagnostic()?,
    ));
    Ok(())
}
