use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_lockfile::LazyLockfile;
use pacquet_network::{ForInstallsError, NetworkSettings, ThrottledClient};
use pacquet_package_manager::ResolvedPackages;
use pacquet_package_manifest::{PackageManifest, PackageManifestError};
use pacquet_tarball::MemCache;
use pipe_trait::Pipe;
use std::{path::PathBuf, sync::Arc};

/// Application state when running `pacquet run` or `pacquet install`.
pub struct State {
    /// Shared cache that stores downloaded tarballs. Held behind
    /// [`Arc`] so the resolve-time prefetch
    /// ([`pacquet_package_manager::PrefetchingResolver`]) can capture
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
    /// The `pnpm-lock.yaml` file, read + parsed on first access so the
    /// repeat-install fast path (which never needs its contents) skips
    /// the YAML parse.
    pub lockfile: LazyLockfile,
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
        let lockfile = if should_load {
            manifest_path
                .parent()
                .expect("manifest path always has a parent dir")
                .to_path_buf()
                .pipe(LazyLockfile::deferred)
        } else {
            LazyLockfile::disabled()
        };
        Ok(State {
            config,
            manifest: manifest_path
                .pipe(PackageManifest::create_if_needed)
                .map_err(InitStateError::Manifest)?,
            lockfile,
            http_client: std::sync::Arc::new(
                ThrottledClient::for_installs(
                    &config.proxy,
                    &config.tls,
                    &config.tls_by_uri,
                    &NetworkSettings {
                        network_concurrency: config.network_concurrency,
                        fetch_timeout: std::time::Duration::from_millis(config.fetch_timeout),
                        user_agent: config.user_agent.clone(),
                    },
                )
                .map_err(InitStateError::Network)?,
            ),
            tarball_mem_cache: Arc::new(MemCache::new()),
            resolved_packages: ResolvedPackages::new(),
        })
    }
}
