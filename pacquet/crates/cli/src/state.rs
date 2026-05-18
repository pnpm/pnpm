use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_lockfile::{LoadLockfileError, Lockfile};
use pacquet_network::{ForInstallsError, ThrottledClient};
use pacquet_package_manager::ResolvedPackages;
use pacquet_package_manifest::{PackageManifest, PackageManifestError};
use pacquet_tarball::MemCache;
use pipe_trait::Pipe;
use std::path::PathBuf;

/// Application state when running `pacquet run` or `pacquet install`.
pub struct State {
    /// Shared cache that store downloaded tarballs.
    pub tarball_mem_cache: MemCache,
    /// HTTP client to make HTTP requests.
    pub http_client: ThrottledClient,
    /// Merged runtime configuration: built-in defaults, with overlays from
    /// the auth subset of `.npmrc` and from `pnpm-workspace.yaml`.
    pub config: &'static Config,
    /// Data from the `package.json` file.
    pub manifest: PackageManifest,
    /// Data from the `pnpm-lock.yaml` file.
    pub lockfile: Option<Lockfile>,
    /// In-memory cache for packages that have started resolving dependencies.
    pub resolved_packages: ResolvedPackages,
}

/// Error type of [`State::init`].
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum InitStateError {
    #[diagnostic(transparent)]
    Manifest(#[error(source)] PackageManifestError),

    #[diagnostic(transparent)]
    Lockfile(#[error(source)] LoadLockfileError),

    #[diagnostic(transparent)]
    Network(#[error(source)] ForInstallsError),
}

impl State {
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
        let should_load = config.lockfile || require_lockfile;
        Ok(State {
            config,
            manifest: manifest_path
                .pipe(PackageManifest::create_if_needed)
                .map_err(InitStateError::Manifest)?,
            lockfile: call_load_lockfile(should_load, Lockfile::load_from_current_dir)
                .map_err(InitStateError::Lockfile)?,
            http_client: ThrottledClient::for_installs(
                &config.proxy,
                &config.tls,
                &config.tls_by_uri,
            )
            .map_err(InitStateError::Network)?,
            tarball_mem_cache: MemCache::new(),
            resolved_packages: ResolvedPackages::new(),
        })
    }
}

/// Load the lockfile from the current directory when `should_load` is
/// `true`. Callers compose `should_load` from `config.lockfile ||
/// --frozen-lockfile` so the CLI flag is always honoured.
///
/// This function was extracted to be tested independently.
fn call_load_lockfile<LoadLockfile, Lockfile, Error>(
    should_load: bool,
    load_lockfile: LoadLockfile,
) -> Result<Option<Lockfile>, Error>
where
    LoadLockfile: FnOnce() -> Result<Option<Lockfile>, Error>,
{
    should_load.then(load_lockfile).transpose().map(Option::flatten)
}

#[cfg(test)]
mod tests;
