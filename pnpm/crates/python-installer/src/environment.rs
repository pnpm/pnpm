use super::{
    Arc, Environments, Index, Inputs, InstallOptions, Interpreter, IntoDiagnostic, Lockfile,
    PYPI_ECOSYSTEM, Path, PathBuf, PnprClient, PypiResolveOptions, Result, StoreIndexWriter, bail,
    fs, io, manifest,
};
use miette::WrapErr;
use std::collections::{BTreeMap, BTreeSet};

/// What every project of one [`prepare`](super::prepare) run shares,
/// before the interpreter each one is installed with is known.
pub(super) struct Shared<'a> {
    pub(super) context: &'a InstallOptions,
    pub(super) index: &'a Index,
    pub(super) store: ArtifactStore<'a>,
    pub(super) asked: Asked,
    /// Every distribution a project in this repository declares. A build
    /// requirement naming one of them is refused rather than taken from
    /// the index, wherever the backend asked for it.
    pub(super) members: BTreeMap<std::path::PathBuf, BTreeSet<pep508_rs::PackageName>>,
    /// The environments backends have already been installed into. Every
    /// project using one backend needs the same environment, and a
    /// workspace is mostly one backend.
    pub(super) build_environments: BuildEnvironments,
}

/// A backend environment belongs to the interpreter that installed it and
/// the requirements it holds: a backend runs in the interpreter it was
/// installed for, and what it compiles is built for that one.
pub(super) type BuildEnvironmentKey = (String, String);

pub(super) type BuildEnvironments =
    tokio::sync::Mutex<BTreeMap<BuildEnvironmentKey, Option<Arc<tempfile::TempDir>>>>;

/// What preparing one project needs: what the run shares, plus the
/// interpreter that installs this project and the environments it is
/// locked for.
pub(super) struct PythonPrepare<'a> {
    pub(super) context: &'a InstallOptions,
    pub(super) interpreter: &'a Interpreter,
    pub(super) environments: &'a Environments,
    pub(super) index: &'a Index,
    pub(super) store: ArtifactStore<'a>,
    pub(super) asked: Asked,
    pub(super) members: &'a BTreeMap<std::path::PathBuf, BTreeSet<pep508_rs::PackageName>>,
    pub(super) build_environments: &'a BuildEnvironments,
}

impl<'a> PythonPrepare<'a> {
    pub(super) fn for_project(
        shared: &'a Shared<'a>,
        interpreter: &'a Interpreter,
        environments: &'a Environments,
    ) -> Self {
        Self {
            context: shared.context,
            interpreter,
            environments,
            index: shared.index,
            store: ArtifactStore { index: shared.store.index.clone(), writer: shared.store.writer },
            asked: shared.asked,
            members: &shared.members,
            build_environments: &shared.build_environments,
        }
    }
}

/// What the install asked this run for.
#[derive(Clone, Copy)]
pub(super) struct Asked {
    /// Whether a dependency is being added, which resolves again.
    pub(super) resolve: bool,
    pub(super) selection: manifest::DependencySelection,
}

/// The store a run reads verified artifacts from and writes them to.
pub(super) struct ArtifactStore<'a> {
    pub(super) index: Option<pnpm_store_dir::SharedReadonlyStoreIndex>,
    pub(super) writer: &'a Arc<StoreIndexWriter>,
}

/// What [`PythonPrepare::lockfile`] needs about one project.
pub(super) struct LockfileInputs<'a> {
    /// The lockfile on disk, when it still applies to these inputs on this
    /// target.
    pub(super) existing: Option<Lockfile>,
    pub(super) lock_path: &'a Path,
    pub(super) requirements: &'a [pep508_rs::Requirement],
    pub(super) inputs: Inputs,
    pub(super) requires_python: Option<String>,
    /// The projects in this repository the resolution installs from their
    /// source, which every seeding of it has to offer again.
    pub(super) local: Arc<[super::workspace::LocalProject]>,
}

/// What [`PythonPrepare::replay_lockfile`] needs about the lockfile on
/// disk.
pub(super) struct LockfileReplay<'a> {
    pub(super) lock: Lockfile,
    pub(super) lock_path: &'a Path,
    pub(super) requirements: &'a [pep508_rs::Requirement],
    pub(super) local: Arc<[super::workspace::LocalProject]>,
    /// Whether the lockfile was resolved for this install's own target,
    /// so that every wheel it pins is one this target needs.
    pub(super) same_target: bool,
}

/// The lockfile beside the project, or `None` when it has none yet.
pub(super) async fn read_existing_lock(lock_path: &Path) -> Result<Option<Lockfile>> {
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

/// Refuse a lockfile that answers a different question than this install
/// asked. Writing one would leave behind a lockfile the next install reads
/// back as stale, and a frozen install would fail on it outright.
pub(super) fn accept_server_lockfile(
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
pub(super) async fn resolve_via_pnpr(
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
    if !pnpm_pnpr_client::server_resolves(&client, pnpr_server, PYPI_ECOSYSTEM)
        .await
        .wrap_err("negotiate Python resolution with the pnpr server")?
    {
        return Ok(None);
    }
    let resolved = client.resolve_pypi(PypiResolveOptions {
        requirements: requirements
            .iter()
            .map(ToString::to_string)
            .collect(),
        target: target.clone(),
        index: index.to_string(),
        requires_python,
        authorization: config.auth_headers.for_url(pnpr_server),
    })
    .await;
    match resolved {
        Err(pnpm_pnpr_client::PnprClientError::Server(message))
            if message.starts_with("Python direct URL requirement for ")
                && message.ends_with(" must be resolved by the client") =>
        {
            Ok(None)
        }
        result => result
            .into_diagnostic()
            .wrap_err("resolve Python dependencies through the pnpr server")
            .map(Some),
    }
}

pub(super) fn ensure_environment_parent(root: &Path) -> Result<()> {
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

pub(super) fn validate_environment_link(root: &Path) -> Result<Option<PathBuf>> {
    let link = root.join(".venv");
    match fs::symlink_metadata(&link) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).into_diagnostic(),
        Ok(_) => {
            if !pnpm_fs::is_symlink_or_junction(&link).into_diagnostic()? {
                bail!("pnpm will not replace an unmanaged Python environment: {}", link.display());
            }
            let target = root.join(pnpm_fs::read_symlink_dir(&link).into_diagnostic()?);
            let target = dunce::canonicalize(&target)
                .into_diagnostic()
                .wrap_err_with(|| {
                    format!(
                        "resolve Python environment target {} for {}",
                        target.display(),
                        link.display(),
                    )
                })?;
            let managed = root.join(".pnpm/python-envs");
            let managed = dunce::canonicalize(&managed)
                .into_diagnostic()
                .wrap_err_with(|| {
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

pub(super) fn publish_link(root: &Path, target: &Path) -> Result<()> {
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
