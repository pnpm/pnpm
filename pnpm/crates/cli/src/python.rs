pub(crate) mod manifest;

mod host;
mod registry;
mod resolver;

use crate::ecosystem_install::{EcosystemManifest, EcosystemWorkspaceInventory, InstallContext};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use host::Interpreter;
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pnpm_pnpr_client::{PYPI_ECOSYSTEM, PnprClient, PypiResolveOptions};
use pnpm_python_resolver::{Inputs, Lockfile};
use pnpm_reporter::Reporter;
use pnpm_store_dir::{StoreIndex, StoreIndexWriter};
use registry::Registry;
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

struct Prepared {
    root: PathBuf,
    lock: String,
    environment: Option<tempfile::TempDir>,
    previous_environment: Option<Option<PathBuf>>,
}

#[derive(Debug, Clone, Copy)]
struct AddOptions<'a> {
    requirements: &'a [String],
    development: bool,
    exact: bool,
    prefix: Option<&'a str>,
}

pub(crate) fn plan_add<Reporter: self::Reporter + 'static>(
    context: InstallContext,
    root: &Path,
    requirements: Vec<String>,
    development: bool,
    exact: bool,
    prefix: Option<String>,
) -> Result<pnpm_install_coordinator::InstallTask<'static>> {
    if !context.config.python.enabled {
        bail!("pypi: dependencies require `python.enabled: true` in pnpm-workspace.yaml");
    }
    let path = root.join("pyproject.toml");
    let metadata = vec![path.clone(), root.join("pylock.toml")];
    let prepare = async move {
        manifest::add(&path, &requirements, development)?;
        let config = context.config;
        let mut prepared =
            prepare::<Reporter>(context, vec![path], true, manifest::DependencySelection::ALL)
                .await?;
        save_added(
            &mut prepared,
            config,
            AddOptions {
                requirements: &requirements,
                development,
                exact,
                prefix: prefix.as_deref(),
            },
        )?;
        Ok(prepared)
    };
    Ok(pnpm_install_coordinator::InstallTask::new(metadata, prepare))
}

fn save_added(
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
    options: AddOptions<'_>,
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

pub(crate) async fn plan<Reporter: self::Reporter + 'static>(
    context: InstallContext,
    inventory: &EcosystemWorkspaceInventory,
    selection: manifest::DependencySelection,
) -> Result<pnpm_install_coordinator::InstallTask<'static>> {
    let manifests = inventory.manifests(EcosystemManifest::Python).await?.to_vec();
    let metadata = manifests.iter().map(|path| path.with_file_name("pylock.toml")).collect();
    Ok(pnpm_install_coordinator::InstallTask::new(
        metadata,
        prepare::<Reporter>(context, manifests, false, selection),
    ))
}

async fn prepare<Reporter: self::Reporter + 'static>(
    context: InstallContext,
    manifests: Vec<PathBuf>,
    resolve: bool,
    selection: manifest::DependencySelection,
) -> Result<Vec<Prepared>> {
    let config = context.config;
    let roots = read_project_manifests(manifests).await?;
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let interpreter: Interpreter =
        host::run(&config.python.executable, "probe", serde_json::json!({})).await?;
    let (index, auth) = python_index(config)?;
    config.store_dir.init().into_diagnostic()?;
    let store_index = StoreIndex::shared_for(&config.store_dir, config.frozen_store);
    let (writer, writer_task) = StoreIndexWriter::spawn_for(&config.store_dir, config.frozen_store);
    let prepare = PythonPrepare {
        context: &context,
        interpreter: &interpreter,
        index: &index,
        auth: &auth,
        store_index,
        writer: &writer,
        resolve,
        selection,
    };
    let result = async {
        let mut prepared = Vec::new();
        for (root, manifest) in roots {
            prepared.push(prepare.project::<Reporter>(root, manifest).await?);
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

/// The projects among `manifests`: a `pyproject.toml` without a
/// `[project]` table declares no package of its own.
async fn read_project_manifests(
    manifests: Vec<PathBuf>,
) -> Result<Vec<(PathBuf, manifest::Manifest)>> {
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
    Ok(roots)
}

/// The configured Python index, with any credentials it carries lifted out
/// of the URL. A repository-selected Python index must not select
/// user-level npm credentials.
fn python_index(config: &pnpm_config::Config) -> Result<(url::Url, pnpm_network::AuthHeaders)> {
    let mut index: url::Url = config.python.index_url.parse().into_diagnostic()?;
    let mut auth = pnpm_network::AuthHeaders::default().with_secure_transport();
    if !index.username().is_empty() || index.password().is_some() {
        let username = pnpm_network::percent_decode_str(index.username());
        let password = pnpm_network::percent_decode_str(index.password().unwrap_or(""));
        index.set_username("").map_err(|()| miette::miette!("invalid Python index URL"))?;
        index.set_password(None).map_err(|()| miette::miette!("invalid Python index URL"))?;
        auth.insert_url_header(
            index.as_str(),
            format!("Basic {}", STANDARD.encode(format!("{username}:{password}"))),
        );
    }
    pnpm_python_resolver::validate_url(&index)?;
    if !index.path().ends_with('/') {
        index.set_path(&format!("{}/", index.path()));
    }
    Ok((index, auth))
}

/// What every project of one [`prepare`] run shares.
struct PythonPrepare<'a> {
    context: &'a InstallContext,
    interpreter: &'a Interpreter,
    index: &'a url::Url,
    auth: &'a pnpm_network::AuthHeaders,
    store_index: Option<pnpm_store_dir::SharedReadonlyStoreIndex>,
    writer: &'a Arc<StoreIndexWriter>,
    resolve: bool,
    selection: manifest::DependencySelection,
}

impl PythonPrepare<'_> {
    async fn project<Reporter: self::Reporter + 'static>(
        &self,
        root: PathBuf,
        manifest: manifest::Manifest,
    ) -> Result<Prepared> {
        let config = self.context.config;
        let project = manifest.project.as_ref().expect("only project manifests were selected");
        self.check_requires_python(&root, project.requires_python.as_deref())?;
        let requirements = manifest.requirements(config, manifest::DependencySelection::ALL)?;
        let inputs = Inputs::new(&requirements, &self.interpreter.target, self.index.as_str());
        let mut registry = self.registry();
        let lock_path = root.join("pylock.toml");
        let existing = read_existing_lock(&lock_path).await?;
        let fresh = existing.as_ref().is_some_and(|lock| {
            lock.tool.pnpm == inputs && lock.requires_python == project.requires_python
        });
        if self.context.frozen_lockfile && (!fresh || self.resolve) {
            bail!("frozen Python lockfile is missing or out of date: {}", lock_path.display());
        }
        let lock = self
            .lockfile::<Reporter>(
                &mut registry,
                LockfileInputs {
                    existing: existing.filter(|_| fresh && !self.resolve),
                    requirements: &requirements,
                    inputs,
                    requires_python: project.requires_python.clone(),
                },
            )
            .await?;
        let environment = self
            .environment(
                &mut registry,
                &root,
                &lock,
                &manifest.requirements(config, self.selection)?,
            )
            .await?;
        Ok(Prepared {
            root,
            lock: toml::to_string_pretty(&lock).into_diagnostic()?,
            environment,
            previous_environment: None,
        })
    }

    fn registry(&self) -> Registry<'_> {
        Registry {
            config: self.context.config,
            client: &self.context.http_client,
            auth: self.auth.clone(),
            index: self.index.clone(),
            interpreter: self.interpreter,
            store_index: self.store_index.clone(),
            writer: Arc::clone(self.writer),
            verified: Arc::default(),
            packages: pnpm_python_resolver::Packages::new(),
            wheels: BTreeMap::new(),
        }
    }

    /// A project that pins an interpreter range cannot be installed with an
    /// interpreter outside it.
    fn check_requires_python(&self, root: &Path, requires_python: Option<&str>) -> Result<()> {
        let Some(specifiers) = requires_python else {
            return Ok(());
        };
        let specifiers: pep440_rs::VersionSpecifiers = specifiers.parse().into_diagnostic()?;
        if !specifiers.contains(self.interpreter.target.environment.python_full_version()) {
            bail!(
                "{} requires Python {specifiers}, but {} was selected",
                root.display(),
                self.interpreter.target.environment.python_full_version(),
            );
        }
        Ok(())
    }

    /// The lockfile for one project: the one on disk when it still matches,
    /// then the one the server resolves, and a local resolution last.
    async fn lockfile<Reporter: self::Reporter + 'static>(
        &self,
        registry: &mut Registry<'_>,
        LockfileInputs { existing, requirements, inputs, requires_python }: LockfileInputs<'_>,
    ) -> Result<Lockfile> {
        if let Some(lock) = existing {
            self.accept_lockfile::<Reporter>(registry, lock, requirements).await
        } else if let Some(lock) = resolve_via_pnpr(
            self.context.config,
            requirements,
            &self.interpreter.target,
            self.index.as_str(),
            requires_python.clone(),
        )
        .await?
        {
            accept_server_lockfile(&lock, &inputs, requires_python.as_deref())?;
            self.accept_lockfile::<Reporter>(registry, lock, requirements).await
        } else {
            let solution = resolver::resolve::<Reporter>(registry, requirements).await?;
            Lockfile::new(
                &registry.packages,
                &self.interpreter.target,
                solution,
                inputs,
                requires_python,
            )
        }
    }

    /// Fetch the wheels a ready-made lockfile pins and check that it still
    /// covers the project's requirements.
    async fn accept_lockfile<Reporter: self::Reporter + 'static>(
        &self,
        registry: &mut Registry<'_>,
        lock: Lockfile,
        requirements: &[pep508_rs::Requirement],
    ) -> Result<Lockfile> {
        lock.seed(&mut registry.packages)?;
        registry.fetch_wheels::<Reporter>(&lock.packages).await?;
        resolver::validate_locked(registry, requirements)?;
        Ok(lock)
    }

    /// Install the locked wheels the project selects into a fresh
    /// environment generation. A lockfile-only run builds none.
    async fn environment(
        &self,
        registry: &mut Registry<'_>,
        root: &Path,
        lock: &Lockfile,
        selected_requirements: &[pep508_rs::Requirement],
    ) -> Result<Option<tempfile::TempDir>> {
        if self.context.lockfile_only {
            return Ok(None);
        }
        validate_environment_link(root)?;
        let generations = root.join(".pnpm/python-envs");
        ensure_environment_parent(root)?;
        let environment =
            tempfile::Builder::new().prefix("env-").tempdir_in(&generations).into_diagnostic()?;
        registry.packages.candidates.clear();
        lock.seed(&mut registry.packages)?;
        let selected = resolver::locked_solution(registry, selected_requirements)?;
        let wheels =
            selected.into_iter().map(|package| &registry.wheels[&package]).collect::<Vec<_>>();
        host::run::<serde_json::Value>(
            &self.interpreter.executable,
            "install",
            serde_json::json!({"root": environment.path(), "packages": wheels}),
        )
        .await?;
        Ok(Some(environment))
    }
}

/// What [`PythonPrepare::lockfile`] needs about one project.
struct LockfileInputs<'a> {
    /// The lockfile on disk, when it is still current for these inputs.
    existing: Option<Lockfile>,
    requirements: &'a [pep508_rs::Requirement],
    inputs: Inputs,
    requires_python: Option<String>,
}

/// The lockfile beside the project, or `None` when it has none yet.
async fn read_existing_lock(lock_path: &Path) -> Result<Option<Lockfile>> {
    let contents = match tokio::fs::read_to_string(lock_path).await {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).into_diagnostic(),
    };
    toml::from_str::<Lockfile>(&contents)
        .into_diagnostic()
        .wrap_err_with(|| format!("parse {}", lock_path.display()))
        .map(Some)
}

impl pnpm_install_coordinator::PreparedInstall for Prepared {
    fn publish(&mut self) -> Result<()> {
        if let Some(environment) = &self.environment {
            self.previous_environment = Some(validate_environment_link(&self.root)?);
            publish_link(&self.root, environment.path())?;
        }
        let lock_path = self.root.join("pylock.toml");
        let previous = match fs::read_to_string(&lock_path) {
            Ok(contents) => Some(contents),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).into_diagnostic(),
        };
        if previous.as_deref() != Some(&self.lock) {
            pnpm_fs::write_atomic(&lock_path, self.lock.as_bytes()).into_diagnostic()?;
        }
        Ok(())
    }

    fn rollback(&mut self) -> Result<()> {
        match &self.previous_environment {
            Some(Some(previous)) => publish_link(&self.root, previous),
            Some(None) => match pnpm_fs::remove_symlink_dir(&self.root.join(".venv")) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error).into_diagnostic(),
            },
            None => Ok(()),
        }
    }

    fn retain(self: Box<Self>) {
        if let Some(environment) = self.environment {
            let _ = environment.keep();
        }
    }
}
/// Refuse a lockfile that answers a different question than this install
/// asked. Writing one would leave behind a lockfile the next install reads
/// back as stale, and a frozen install would fail on it outright.
fn accept_server_lockfile(
    lock: &Lockfile,
    inputs: &Inputs,
    requires_python: Option<&str>,
) -> Result<()> {
    if lock.tool.pnpm != *inputs || lock.requires_python.as_deref() != requires_python {
        bail!("the pnpr server resolved Python dependencies for other inputs");
    }
    Ok(())
}

/// Resolve through the configured pnpr server, which reads the index and
/// each wheel's metadata instead of making this client download wheels to
/// find out what they require.
///
/// `None` when there is no server to ask, or when the one configured
/// resolves Python not at all.
async fn resolve_via_pnpr(
    config: &pnpm_config::Config,
    requirements: &[pep508_rs::Requirement],
    target: &pnpm_python_resolver::Target,
    index: &str,
    requires_python: Option<String>,
) -> Result<Option<Lockfile>> {
    let Some(pnpr_server) = config.pnpr_server.as_deref().filter(|_| !config.offline) else {
        return Ok(None);
    };
    let client = PnprClient::new(pnpr_server);
    if !crate::pnpr_ecosystems::server_resolves(&client, pnpr_server, PYPI_ECOSYSTEM)
        .await
        .wrap_err("negotiate Python resolution with the pnpr server")?
    {
        return Ok(None);
    }
    client
        .resolve_pypi(PypiResolveOptions {
            requirements: requirements.iter().map(ToString::to_string).collect(),
            target: target.clone(),
            index: index.to_string(),
            requires_python,
            authorization: config.auth_headers.for_url(pnpr_server),
        })
        .await
        .into_diagnostic()
        .wrap_err("resolve Python dependencies through the pnpr server")
        .map(Some)
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
            let target = dunce::canonicalize(&target).into_diagnostic().wrap_err_with(|| {
                format!(
                    "resolve Python environment target {} for {}",
                    target.display(),
                    link.display(),
                )
            })?;
            let managed = root.join(".pnpm/python-envs");
            let managed = dunce::canonicalize(&managed).into_diagnostic().wrap_err_with(|| {
                format!(
                    "resolve managed Python directory {} for {}",
                    managed.display(),
                    link.display(),
                )
            })?;
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

#[cfg(test)]
mod tests;
