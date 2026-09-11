use super::{InstallOptions, Lockfile, Prepared, Reporter, manifest, prepare};
use miette::{IntoDiagnostic, Result, bail};
use std::{fs, path::Path};

#[derive(Debug, Clone)]
pub struct AddOptions {
    pub requirements: Vec<String>,
    pub development: bool,
    pub exact: bool,
    pub prefix: Option<String>,
}

pub fn plan_add<Reporter: self::Reporter + 'static>(
    context: InstallOptions,
    root: &Path,
    options: AddOptions,
) -> Result<pnpm_install_coordinator::InstallTask<'static>> {
    if !context.config.python.enabled {
        bail!("pypi: dependencies require `python.enabled: true` in pnpm-workspace.yaml");
    }
    if !matches!(options.prefix.as_deref().unwrap_or(">="), ">=" | "~=" | "==") {
        bail!("Python --save-prefix must be >=, ~=, or ==");
    }
    let path = root.join("pyproject.toml");
    let metadata = vec![path.clone(), root.join("pylock.toml")];
    let prepare = async move {
        manifest::add(&path, &options.requirements, options.development)?;
        let config = context.config;
        let mut prepared =
            prepare::<Reporter>(context, vec![path], true, manifest::DependencySelection::ALL)
                .await?;
        save_added(&mut prepared, config, &options)?;
        Ok(prepared)
    };
    Ok(pnpm_install_coordinator::InstallTask::new(metadata, prepare))
}

fn save_added(
    prepared: &mut [Prepared],
    config: &pnpm_config::Config,
    options: &AddOptions,
) -> Result<()> {
    let prefix = options.prefix.as_deref().unwrap_or(">=");
    let [project] = prepared else { bail!("Python add requires exactly one project") };
    let mut lock: Lockfile = toml::from_str(&project.lock).into_diagnostic()?;
    let mut requirements = Vec::new();
    for requirement in &options.requirements {
        let mut requirement = pnpm_python_resolver::parse_requirement(requirement)?;
        pin_to_locked_version(&mut requirement, &lock, options, prefix)?;
        requirements.push(requirement.to_string());
    }
    let path = project.root.join("pyproject.toml");
    manifest::add(&path, &requirements, options.development)?;
    let manifest = manifest::Manifest::parse(&fs::read_to_string(path).into_diagnostic()?)?;
    lock.tool
        .pnpm
        .set_requirements(&manifest.requirements(config, manifest::DependencySelection::ALL)?);
    project.lock = toml::to_string_pretty(&lock).into_diagnostic()?;
    Ok(())
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
    let Some(package) = lock.packages.iter().find(|package| package.name == requirement.name)
    else {
        return Ok(());
    };
    let prefix = if options.exact { "==" } else { prefix };
    requirement.version_or_url = Some(pep508_rs::VersionOrUrl::VersionSpecifier(
        format!("{prefix}{}", package.version).parse().into_diagnostic()?,
    ));
    Ok(())
}
