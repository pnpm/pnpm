use derive_more::{Display, Error};
use miette::Diagnostic;
use pipe_trait::Pipe;
use pnpm_config::Config;
use pnpm_lockfile::{LazyLockfile, MaybeLazyLockfile};
use pnpm_network::{ForInstallsError, ThrottledClient};
use pnpm_package_is_installable::{Engine, InstallabilityError, WantedEngine, check_engine};
use pnpm_package_manager::{CommandLockfile, ResolvedPackages};
use pnpm_package_manifest::{
    PackageManifest, PackageManifestError, node_version_from_engines_runtime,
};
use pnpm_tarball::MemCache;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Application state when running `pacquet run` or `pacquet install`.
pub struct State {
    /// Shared cache that stores downloaded tarballs. Held behind
    /// [`Arc`] so the resolve-time prefetch
    /// ([`pnpm_package_manager::PrefetchingResolver`]) can capture
    /// an owned clone into the `tokio::spawn`ed background download
    /// while every install sub-pipeline still takes a borrowed
    /// `&MemCache` via deref.
    pub tarball_mem_cache: Arc<MemCache>,
    /// HTTP client to make HTTP requests. Held behind [`std::sync::Arc`] so
    /// the lockfile-verification gate can own a clone for the
    /// `NpmResolutionVerifier`'s lifetime while every install
    /// sub-pipeline takes a borrowed `&ThrottledClient` via deref.
    pub http_client: std::sync::Arc<ThrottledClient>,
    /// Merged runtime configuration: built-in defaults, with overlays from
    /// the auth subset of `.npmrc` and from `pnpm-workspace.yaml`.
    pub config: &'static Config,
    /// Data from the `package.json` file.
    pub manifest: PackageManifest,
    /// The wanted lockfile, read + parsed on first access so the
    /// repeat-install fast path (which never needs its contents) skips
    /// the YAML parse.
    pub lockfile: LazyLockfile,
    /// In-memory cache for packages that have started resolving dependencies.
    pub resolved_packages: ResolvedPackages,
}

/// The wanted lockfile as a manifest-mutating command receives it: the
/// document, resolved once here so the command's own reads stay
/// infallible, together with the loader that produced it and its path.
///
/// The loader travels on to the install, which is what lets the install
/// report and gate on a Git-conflict merge this load performed.
/// Reporting here instead would announce a merge before anything knows
/// whether the run writes the file back.
pub(crate) fn command_lockfile<'a>(
    lockfile: &'a LazyLockfile,
    path: &'a Path,
) -> miette::Result<CommandLockfile<'a>> {
    let source = MaybeLazyLockfile::Lazy(lockfile);
    let document = source
        .get()
        .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;
    Ok(CommandLockfile { document, source, path: Some(path) })
}

/// Error type of [`State::init`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum InitStateError {
    #[diagnostic(transparent)]
    Manifest(#[error(source)] PackageManifestError),

    #[diagnostic(transparent)]
    ManifestRead(#[error(source)] pnpm_workspace::ReadProjectManifestOnlyError),

    #[diagnostic(transparent)]
    Network(#[error(source)] ForInstallsError),

    #[diagnostic(transparent)]
    Installability(#[error(source)] Box<InstallabilityError>),
}

impl State {
    pub(crate) fn install<Groups>(
        &self,
        dependency_groups: Groups,
    ) -> pnpm_package_manager::Install<'_, Groups>
    where
        Groups: IntoIterator<Item = pnpm_package_manifest::DependencyGroup>,
    {
        pnpm_package_manager::Install::new(
            Arc::clone(&self.tarball_mem_cache),
            &self.resolved_packages,
            (&self.http_client, Arc::clone(&self.http_client)),
            self.config,
            &self.manifest,
            pnpm_lockfile::MaybeLazyLockfile::Lazy(&self.lockfile),
            dependency_groups,
        )
    }

    /// Initialize the application state.
    ///
    /// `require_lockfile` is `true` when the caller has committed to the
    /// frozen-lockfile install path (via `--frozen-lockfile`) and needs
    /// the lockfile loaded even when `config.lockfile` is `false`.
    /// Matches pnpm's CLI: `--frozen-lockfile` is the strongest signal,
    /// it must not be silently dropped because `lockfile` is disabled
    /// (or unset) in config.
    pub fn init(
        manifest_path: PathBuf,
        config: &'static Config,
        require_lockfile: bool,
    ) -> Result<Self, InitStateError> {
        let lockfile = Self::lazy_lockfile(config, &manifest_path, require_lockfile);
        Self::init_with_lockfile(manifest_path, config, lockfile)
    }

    /// The lazy wanted lockfile [`Self::init`] would build. Split out
    /// so a caller with work between here and the install — project
    /// discovery above all — can build it first and start its
    /// [`LazyLockfile::prefetch`] over that work, handing the result to
    /// [`Self::init_with_lockfile`].
    #[must_use]
    pub fn lazy_lockfile(
        config: &Config,
        manifest_path: &Path,
        require_lockfile: bool,
    ) -> LazyLockfile {
        let should_load = config.lockfile || require_lockfile;
        if should_load {
            manifest_path
                .parent()
                .expect("manifest path always has a parent dir")
                .pipe(|manifest_dir| config.lockfile_dir_for(manifest_dir))
                .to_path_buf()
                .pipe(|dir| LazyLockfile::deferred(dir, config.wanted_lockfile_selection()))
        } else {
            LazyLockfile::disabled()
        }
    }

    /// [`Self::init`] with a caller-built [`Self::lazy_lockfile`].
    pub fn init_with_lockfile(
        manifest_path: PathBuf,
        config: &'static Config,
        lockfile: LazyLockfile,
    ) -> Result<Self, InitStateError> {
        let http_client = Self::new_http_client(config)?;
        Self::init_with_lockfile_and_http_client(manifest_path, config, lockfile, http_client)
    }

    /// Build the install-wide HTTP client shared by every ecosystem.
    pub(crate) fn new_http_client(config: &Config) -> Result<Arc<ThrottledClient>, InitStateError> {
        ThrottledClient::for_installs(
            &config.proxy,
            &config.tls,
            &config.tls_by_uri,
            &config.network_settings(),
        )
        .map(|client| Arc::new(client.with_max_sockets_per_host(config.max_sockets)))
        .map_err(InitStateError::Network)
    }

    /// [`Self::init_with_lockfile`] with a caller-owned install-wide HTTP client.
    pub(crate) fn init_with_lockfile_and_http_client(
        manifest_path: PathBuf,
        config: &'static Config,
        lockfile: LazyLockfile,
        http_client: Arc<ThrottledClient>,
    ) -> Result<Self, InitStateError> {
        let manifest = load_or_create_manifest(manifest_path, config)?;
        Ok(State {
            config,
            manifest,
            lockfile,
            http_client,
            tarball_mem_cache: Arc::new(MemCache::new()),
            resolved_packages: ResolvedPackages::new(),
        })
    }

    /// The directory of the project the command runs in — where its
    /// `package.json` lives.
    pub fn project_dir(&self) -> &Path {
        self.manifest
            .path()
            .parent()
            .expect("manifest path always has a parent dir")
    }

    pub fn lockfile_dir(&self) -> &Path {
        self.config.lockfile_dir_for(self.project_dir())
    }

    pub fn lockfile_path(&self) -> PathBuf {
        self.lockfile_dir().join(self.config.wanted_lockfile_name())
    }

    pub fn active_importer_id(&self) -> String {
        pnpm_workspace::importer_id_from_root_dir(self.lockfile_dir(), self.project_dir())
    }
}

/// `package.json` loads (or is scaffolded) as usual, but when it is
/// absent an existing alternate manifest base name (`package.yaml`)
/// must be loaded rather than shadowed by a scaffolded `package.json`
/// and saved in its original format.
///
/// Inside a workspace, a missing root manifest is tolerated rather
/// than scaffolded: pnpm installs such a workspace with no root
/// importer and never creates a `package.json` there, so the stand-in
/// manifest stays in memory only. Commands that save the manifest
/// (`add`) still persist it through their explicit save.
fn load_or_create_manifest(
    manifest_path: PathBuf,
    config: &Config,
) -> Result<PackageManifest, InitStateError> {
    if !manifest_path.exists() {
        let project_dir = manifest_path.parent().expect("manifest path always has a parent dir");
        if let Some((_, manifest)) = pnpm_workspace::try_read_project_manifest(
            project_dir,
            Some(config.preferred_manifest_format),
        )
        .map_err(InitStateError::ManifestRead)?
        {
            return Ok(apply_runtime_on_fail(manifest, config));
        }
        if config.workspace_dir.is_some() {
            return Ok(apply_runtime_on_fail(
                PackageManifest::from_value(manifest_path, serde_json::json!({})),
                config,
            ));
        }
    }
    manifest_path
        .pipe(PackageManifest::create_if_needed)
        .map(|manifest| apply_runtime_on_fail(manifest, config))
        .map_err(InitStateError::Manifest)
}

fn apply_runtime_on_fail(mut manifest: PackageManifest, config: &Config) -> PackageManifest {
    if let Some(runtime_on_fail) = config.runtime_on_fail {
        pnpm_package_manifest::apply_runtime_on_fail_override(
            manifest.value_mut(),
            runtime_on_fail.as_str(),
        );
    }
    manifest
}

pub(crate) fn check_root_project_engine(
    manifest_path: &Path,
    config: &Config,
    use_manifest_runtime: bool,
) -> Result<(), InitStateError> {
    if !config.engine_strict {
        return Ok(());
    }
    let project_dir = config.workspace_dir
        .as_deref()
        .unwrap_or_else(|| manifest_path.parent().expect("manifest path always has a parent dir"));
    let Some((_, manifest)) = pnpm_workspace::try_read_project_manifest(
        project_dir,
        Some(config.preferred_manifest_format),
    )
    .map_err(InitStateError::ManifestRead)?
    else {
        return Ok(());
    };
    let Some(wanted_node) = manifest
        .value()
        .get("engines")
        .and_then(|engines| engines.get("node"))
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(());
    };
    let configured_node = config.node_version
        .clone()
        .or_else(|| {
            use_manifest_runtime
                .then(|| node_version_from_engines_runtime(manifest.value()))
                .flatten()
        });
    let host = pnpm_deps_restorer::InstallabilityHost::detect_with(true, configured_node);
    let wanted = WantedEngine { node: Some(wanted_node.to_string()), pnpm: None };
    let current = Engine { node: host.node_version, pnpm: None };
    match check_engine(&project_dir.to_string_lossy(), &wanted, &current) {
        Ok(None) => Ok(()),
        Ok(Some(error)) => {
            Err(InitStateError::Installability(Box::new(InstallabilityError::Engine(error))))
        }
        Err(error) => Err(InitStateError::Installability(Box::new(
            InstallabilityError::InvalidNodeVersion(error),
        ))),
    }
}

#[cfg(test)]
mod tests;
