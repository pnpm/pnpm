pub(super) use store::{EnvironmentStore, Generation, publish_link};

use super::{
    Arc, Environments, Index, Inputs, InstallOptions, Interpreter, IntoDiagnostic, Lockfile,
    PYPI_ECOSYSTEM, Path, PnprClient, PypiResolveOptions, Result, StoreIndexWriter, bail, io,
    manifest,
};
use miette::WrapErr;
use std::collections::{BTreeMap, BTreeSet};

mod store;

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
    pub(super) caches: Caches,
}

#[derive(Default)]
pub(super) struct Caches {
    /// The environments backends have already been installed into. Every
    /// project using one backend needs the same environment, and a
    /// workspace is mostly one backend.
    pub(super) build_environments: BuildEnvironments,
    pub(super) resolutions: super::resolutions::Resolutions,
}

/// A backend environment belongs to the interpreter that installed it and
/// the requirements it holds: a backend runs in the interpreter it was
/// installed for, and what it compiles is built for that one.
pub(super) type BuildEnvironmentKey = (String, String);

type BuildEnvironmentEntry = Arc<tokio::sync::Mutex<Option<Arc<tempfile::TempDir>>>>;

pub(super) type BuildEnvironments =
    tokio::sync::Mutex<BTreeMap<BuildEnvironmentKey, BuildEnvironmentEntry>>;

/// What preparing one project needs: what the run shares, plus the
/// interpreter that installs this project and the environments it is
/// locked for.
#[derive(Clone)]
pub(super) struct PythonPrepare<'a> {
    pub(super) context: &'a InstallOptions,
    pub(super) interpreter: &'a Interpreter,
    pub(super) environments: &'a Environments,
    pub(super) index: &'a Index,
    pub(super) store: ArtifactStore<'a>,
    pub(super) asked: Asked,
    pub(super) members: &'a BTreeMap<std::path::PathBuf, BTreeSet<pep508_rs::PackageName>>,
    pub(super) state: PreparationState<'a>,
}

#[derive(Clone)]
pub(super) struct PreparationState<'a> {
    pub(super) caches: &'a Caches,
    pub(super) building: BTreeSet<BuildEnvironmentKey>,
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
            state: PreparationState { caches: &shared.caches, building: BTreeSet::new() },
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
#[derive(Clone)]
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
    /// The projects the requirements were collected from, which a failed
    /// resolution is explained in terms of.
    pub(super) members: &'a [super::projects::Member],
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
        // A server answers from an index and the metadata published
        // beside a wheel. What needs this machine instead — an explicit
        // source, or a release whose metadata is in the wheel its source
        // distribution builds — is resolved here.
        Err(pnpm_pnpr_client::PnprClientError::Server(message))
            if message.starts_with("Python ")
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
