mod host;
mod lockfile;
pub(crate) mod manifest;
mod registry;
mod resolver;

use crate::ecosystem_install::{EcosystemManifest, EcosystemWorkspaceInventory, InstallContext};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use host::Interpreter;
use lockfile::{Inputs, Lockfile};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pnpm_reporter::Reporter;
use pnpm_store_dir::{StoreIndex, StoreIndexWriter};
use registry::Registry;
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) enum Projects<'a> {
    Root(&'a Path),
    Workspace(&'a EcosystemWorkspaceInventory),
}

pub(crate) struct InstallOptions<'a> {
    pub(crate) projects: Projects<'a>,
    pub(crate) resolve: bool,
    pub(crate) selection: manifest::DependencySelection,
}

pub(crate) struct Prepared {
    root: PathBuf,
    lock: String,
    environment: Option<tempfile::TempDir>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AddOptions<'a> {
    pub(crate) requirements: &'a [String],
    pub(crate) development: bool,
    pub(crate) exact: bool,
    pub(crate) prefix: Option<&'a str>,
}

pub(crate) fn save_added(
    prepared: &mut [Prepared],
    config: &pnpm_config::Config,
    options: AddOptions<'_>,
) -> Result<()> {
    let prefix = options.prefix.unwrap_or(">=");
    if !matches!(prefix, ">=" | "~=" | "==") {
        bail!("Python --save-prefix must be >=, ~=, or ==");
    }
    let [project] = prepared else { bail!("Python add requires exactly one project") };
    let mut lock: Lockfile = toml::from_str(&project.lock).into_diagnostic()?;
    let mut requirements = Vec::new();
    for requirement in options.requirements {
        let mut requirement = manifest::parse_requirement(requirement)?;
        if (options.exact || requirement.version_or_url.is_none())
            && let Some(package) =
                lock.packages.iter().find(|package| package.name == requirement.name)
        {
            let prefix = if options.exact { "==" } else { prefix };
            requirement.version_or_url = Some(pep508_rs::VersionOrUrl::VersionSpecifier(
                format!("{prefix}{}", package.version).parse().into_diagnostic()?,
            ));
        }
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

pub(crate) async fn prepare<Reporter: self::Reporter + 'static>(
    context: InstallContext,
    options: InstallOptions<'_>,
) -> Result<Vec<Prepared>> {
    let InstallOptions { projects, resolve, selection } = options;
    let config = context.config;
    if !config.python.enabled {
        return Ok(Vec::new());
    }
    let manifests = match projects {
        Projects::Root(root) => vec![root.join("pyproject.toml")],
        Projects::Workspace(inventory) => {
            inventory.manifests(EcosystemManifest::Python).await?.to_vec()
        }
    };
    let mut roots = Vec::new();
    for path in manifests {
        let contents = tokio::fs::read_to_string(&path)
            .await
            .into_diagnostic()
            .wrap_err_with(|| format!("read {}", path.display()))?;
        let manifest = manifest::Manifest::parse(&contents)?;
        if manifest.project.is_some() {
            roots.push((path.parent().expect("manifest has a parent").to_path_buf(), manifest));
        }
    }
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let interpreter: Interpreter =
        host::run(&config.python.executable, "probe", serde_json::json!({})).await?;
    let mut index: url::Url = config.python.index_url.parse().into_diagnostic()?;
    let mut auth = (*config.auth_headers).clone();
    if !index.username().is_empty() || index.password().is_some() {
        let username = percent_decode(index.username())?;
        let password = percent_decode(index.password().unwrap_or(""))?;
        index.set_username("").map_err(|()| miette::miette!("invalid Python index URL"))?;
        index.set_password(None).map_err(|()| miette::miette!("invalid Python index URL"))?;
        auth.insert_url_header(
            index.as_str(),
            format!("Basic {}", STANDARD.encode(format!("{username}:{password}"))),
        );
    }
    registry::validate_url(&index)?;
    if !index.path().ends_with('/') {
        index.set_path(&format!("{}/", index.path()));
    }
    config.store_dir.init().into_diagnostic()?;
    let store_index = StoreIndex::shared_for(&config.store_dir, config.frozen_store);
    let (writer, writer_task) = StoreIndexWriter::spawn_for(&config.store_dir, config.frozen_store);
    let result = async {
        let mut prepared = Vec::new();
        for (root, manifest) in roots {
            let project = manifest.project.as_ref().expect("only project manifests were selected");
            if let Some(specifiers) = &project.requires_python {
                let specifiers: pep440_rs::VersionSpecifiers =
                    specifiers.parse().into_diagnostic()?;
                if !specifiers.contains(interpreter.environment.python_full_version()) {
                    bail!(
                        "{} requires Python {specifiers}, but {} was selected",
                        root.display(),
                        interpreter.environment.python_full_version(),
                    );
                }
            }
            let requirements = manifest.requirements(config, manifest::DependencySelection::ALL)?;
            let inputs = Inputs::new(&requirements, &interpreter, index.as_str());
            let mut registry = Registry {
                config,
                client: &context.http_client,
                auth: auth.clone(),
                index: index.clone(),
                interpreter: &interpreter,
                store_index: store_index.clone(),
                writer: Arc::clone(&writer),
                verified: Arc::default(),
                candidates: BTreeMap::new(),
                wheels: BTreeMap::new(),
            };
            let lock_path = root.join("pylock.toml");
            let existing = match tokio::fs::read_to_string(&lock_path).await {
                Ok(contents) => Some(
                    toml::from_str::<Lockfile>(&contents)
                        .into_diagnostic()
                        .wrap_err_with(|| format!("parse {}", lock_path.display()))?,
                ),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error).into_diagnostic(),
            };
            let fresh = existing.as_ref().is_some_and(|lock| {
                lock.tool.pnpm == inputs && lock.requires_python == project.requires_python
            });
            if context.frozen_lockfile && (!fresh || resolve) {
                bail!("frozen Python lockfile is missing or out of date: {}", lock_path.display());
            }
            let lock = if fresh && !resolve {
                let lock = existing.expect("fresh lockfile exists");
                lock.seed(&mut registry)?;
                for package in &lock.packages {
                    registry.fetch_wheel::<Reporter>(&package.name, &package.version).await?;
                }
                resolver::validate_locked(&registry, &requirements)?;
                lock
            } else {
                let solution = resolver::resolve::<Reporter>(&mut registry, &requirements).await?;
                Lockfile::new(&registry, solution, inputs, project.requires_python.clone())?
            };
            let environment = if context.lockfile_only {
                None
            } else {
                validate_environment_link(&root)?;
                let generations = root.join(".pnpm/python-envs");
                ensure_environment_parent(&root)?;
                let environment = tempfile::Builder::new()
                    .prefix("env-")
                    .tempdir_in(&generations)
                    .into_diagnostic()?;
                registry.candidates.clear();
                lock.seed(&mut registry)?;
                let selected = resolver::locked_solution(
                    &registry,
                    &manifest.requirements(config, selection)?,
                )?;
                let wheels = selected
                    .into_iter()
                    .map(|package| &registry.wheels[&package])
                    .collect::<Vec<_>>();
                host::run::<serde_json::Value>(
                    &interpreter.executable,
                    "install",
                    serde_json::json!({"root": environment.path(), "packages": wheels}),
                )
                .await?;
                Some(environment)
            };
            prepared.push(Prepared {
                root,
                lock: toml::to_string_pretty(&lock).into_diagnostic()?,
                environment,
            });
        }
        Ok(prepared)
    }
    .await;
    drop(writer);
    writer_task
        .await
        .into_diagnostic()
        .wrap_err("join Python artifact store index writer")?
        .into_diagnostic()
        .wrap_err("flush Python artifact store index")?;
    result
}

pub(crate) fn publish(prepared: Vec<Prepared>) -> Result<()> {
    if prepared.is_empty() {
        return Ok(());
    }
    let mut published = Vec::new();
    let outcome = (|| {
        for project in &prepared {
            if let Some(environment) = &project.environment {
                let previous = validate_environment_link(&project.root)?;
                published.push((&project.root, previous));
                publish_link(&project.root, environment.path())?;
            }
            let lock_path = project.root.join("pylock.toml");
            let previous = match fs::read_to_string(&lock_path) {
                Ok(contents) => Some(contents),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error).into_diagnostic(),
            };
            if previous.as_deref() != Some(&project.lock) {
                pnpm_fs::write_atomic(&lock_path, project.lock.as_bytes()).into_diagnostic()?;
            }
        }
        Ok(())
    })();
    if outcome.is_err() {
        let mut rollback_error = None;
        for (root, previous) in published.into_iter().rev() {
            let restored = if let Some(previous) = previous {
                publish_link(root, &previous)
            } else {
                match pnpm_fs::remove_symlink_dir(&root.join(".venv")) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(error).into_diagnostic(),
                }
            };
            if let Err(error) = restored {
                rollback_error = Some(error);
            }
        }
        if let Some(error) = rollback_error {
            for project in prepared {
                if let Some(environment) = project.environment {
                    let _ = environment.keep();
                }
            }
            return Err(error.wrap_err(
                "Python environment rollback failed; retained generations for recovery",
            ));
        }
    }
    outcome?;
    for project in prepared {
        if let Some(environment) = project.environment {
            let _ = environment.keep();
        }
    }
    Ok(())
}

fn ensure_environment_parent(root: &Path) -> Result<()> {
    let mut path = root.to_path_buf();
    for component in [".pnpm", "python-envs"] {
        path.push(component);
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error).into_diagnostic(),
        }
        if !fs::symlink_metadata(&path).into_diagnostic()?.is_dir()
            || pnpm_fs::is_symlink_or_junction(&path).into_diagnostic()?
        {
            bail!("managed Python directory must be a real directory: {}", path.display());
        }
    }
    Ok(())
}

fn validate_environment_link(root: &Path) -> Result<Option<PathBuf>> {
    let link = root.join(".venv");
    match fs::symlink_metadata(&link) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).into_diagnostic(),
        Ok(_) => {
            if !pnpm_fs::is_symlink_or_junction(&link).into_diagnostic()? {
                bail!("pnpm will not replace an unmanaged Python environment: {}", link.display());
            }
            let target = root.join(pnpm_fs::read_symlink_dir(&link).into_diagnostic()?);
            let target = dunce::canonicalize(target).into_diagnostic()?;
            let managed = dunce::canonicalize(root.join(".pnpm/python-envs")).into_diagnostic()?;
            if target.parent() != Some(managed.as_path()) {
                bail!("pnpm will not replace an unmanaged Python environment: {}", link.display());
            }
            Ok(Some(target))
        }
    }
}

fn publish_link(root: &Path, target: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        let outcome = pnpm_fs::force_symlink_dir(target, &root.join(".venv")).into_diagnostic()?;
        if let Some(warning) = outcome.warning {
            bail!("{warning}");
        }
        Ok(())
    }
    #[cfg(unix)]
    {
        let temporary = tempfile::Builder::new()
            .prefix(".pnpm-python-link-")
            .tempdir_in(root)
            .into_diagnostic()?;
        let staged = temporary.path().join(".venv");
        // The link is moved up one level when published, so relative links must
        // be computed from their final location, not from the temporary directory.
        std::os::unix::fs::symlink(target, &staged).into_diagnostic()?;
        fs::rename(&staged, root.join(".venv")).into_diagnostic()
    }
}

fn percent_decode(value: &str) -> Result<String> {
    url::form_urlencoded::parse(format!("value={}", value.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, value)| value.into_owned())
        .ok_or_else(|| miette::miette!("invalid Python index credential"))
}

pub(crate) fn execution_paths<'a>(
    config: &'a pnpm_config::Config,
    dir: &Path,
) -> std::borrow::Cow<'a, [PathBuf]> {
    if !config.python.enabled {
        return std::borrow::Cow::Borrowed(&config.extra_bin_paths);
    }
    let mut paths = vec![dir.join(if cfg!(windows) { ".venv/Scripts" } else { ".venv/bin" })];
    paths.extend(config.extra_bin_paths.iter().cloned());
    std::borrow::Cow::Owned(paths)
}
