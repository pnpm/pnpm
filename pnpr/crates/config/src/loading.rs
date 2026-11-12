use super::{
    Arc, Config, ConfigFile, ConfigSource, DEFAULT_CONFIG_YAML, EnvVar, FeatureOverrides,
    HostedStoreConfig, Path, PathBuf, RegistryError, ResolvedFileRegistries, RoutesFile,
    SocketAddr, SystemEnv, build_features, build_identity_config, build_log_config,
    build_osv_config, build_route_policy, config_file_in, http_config::build_http_config,
    resolve_file_registries, resolve_secrets, resolve_storage_paths,
};
use crate::config_file::RegistryFile;

impl Config {
    /// Load YAML from `path` and merge it with runtime values
    /// supplied by the binary. `listen` and `public_url` are not
    /// represented in verdaccio's YAML and must be provided here;
    /// `packument_ttl` defaults to [`Self::DEFAULT_PACKUMENT_TTL`].
    ///
    /// `storage` from the YAML is resolved relative to the config
    /// file's parent directory when not absolute — same convention
    /// verdaccio uses for `./storage`.
    pub fn from_yaml(
        path: &Path,
        listen: SocketAddr,
        public_url: Option<String>,
    ) -> std::io::Result<Self> {
        Self::from_yaml_with_overrides(path, listen, public_url, FeatureOverrides::default())
    }

    pub(super) fn from_yaml_with_overrides(
        path: &Path,
        listen: SocketAddr,
        public_url: Option<String>,
        overrides: FeatureOverrides,
    ) -> std::io::Result<Self> {
        let raw = std::fs::read_to_string(path)
            .map_err(|err| {
                std::io::Error::new(err.kind(), format!("read {}: {err}", path.display()))
            })?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        Self::from_yaml_str_with_overrides(&raw, base, listen, public_url, overrides)
            .map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("parse {}: {err}", path.display()),
                )
            })
    }

    /// Parse [`DEFAULT_CONFIG_YAML`] (the verdaccio-shaped YAML
    /// bundled into the binary) and merge it with the given runtime
    /// values. Relative `storage:` paths in the bundled YAML are
    /// resolved against `base_dir` — pass `Path::new(".")` to mirror
    /// verdaccio's CWD-relative behaviour, or an absolute path when
    /// the caller knows where the storage should live.
    ///
    /// Panics if the bundled YAML fails to parse — that would be a
    /// build-time bug since the file is compiled in.
    #[must_use]
    pub fn from_default_yaml(
        base_dir: &Path,
        listen: SocketAddr,
        public_url: Option<String>,
    ) -> Self {
        // With default (no) overrides the bundled config keeps both
        // surfaces enabled, so the only way this errors is a malformed
        // compiled-in YAML — a build-time bug, hence the `expect`. The
        // override-taking variant returns `Result` because overrides can
        // disable every surface (a runtime input error).
        Self::from_default_yaml_with_overrides(
            base_dir,
            listen,
            public_url,
            FeatureOverrides::default(),
        )
        .expect("bundled DEFAULT_CONFIG_YAML must always parse")
    }

    pub(super) fn from_default_yaml_with_overrides(
        base_dir: &Path,
        listen: SocketAddr,
        public_url: Option<String>,
        overrides: FeatureOverrides,
    ) -> Result<Self, RegistryError> {
        Self::from_yaml_str_with_overrides(
            DEFAULT_CONFIG_YAML,
            base_dir,
            listen,
            public_url,
            overrides,
        )
    }

    /// Resolve the auto-discovery path for the global `config.yaml`,
    /// reading the process environment. Returns the path only when it
    /// exists as a file; otherwise `None`, so the caller falls back to
    /// [`Self::from_default_yaml`] for the bundled config.
    ///
    /// The directory follows pnpm's own global-config-dir rules (via
    /// the shared [`pnpm_config_dir::config_dir`]) under a `pnpr`
    /// leaf, so an operator who knows where `pnpm config` looks knows
    /// where pnpr looks too.
    pub fn auto_config_path() -> Option<PathBuf> {
        let dir = pnpm_config_dir::config_dir(
            "pnpr",
            std::env::consts::OS,
            std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
            std::env::var("LOCALAPPDATA").ok().as_deref(),
            home::home_dir,
        );
        config_file_in(dir)
    }

    /// Pick the right config source in precedence order:
    /// 1. `explicit` (the binary's `-c` / `--config` flag);
    /// 2. `default_path` (typically [`Self::auto_config_path`]'s
    ///    result);
    /// 3. the bundled [`DEFAULT_CONFIG_YAML`].
    ///
    /// Returns the resolved [`Config`] alongside a [`ConfigSource`]
    /// describing which branch fired so the binary can log it
    /// after the subscriber is up.
    pub fn resolve(
        explicit: Option<&Path>,
        default_path: Option<&Path>,
        listen: SocketAddr,
        public_url: Option<String>,
    ) -> std::io::Result<(Self, ConfigSource)> {
        Self::resolve_with_overrides(
            explicit,
            default_path,
            listen,
            public_url,
            FeatureOverrides::default(),
        )
    }

    /// Like [`Self::resolve`] but applies CLI [`FeatureOverrides`] during
    /// parse, so a surface disabled on the command line skips its parse-time
    /// work (e.g. strict upstream token resolution) — not just its routes. The
    /// binary uses this; tests and embedders that don't override features
    /// call [`Self::resolve`].
    pub fn resolve_with_overrides(
        explicit: Option<&Path>,
        default_path: Option<&Path>,
        listen: SocketAddr,
        public_url: Option<String>,
        overrides: FeatureOverrides,
    ) -> std::io::Result<(Self, ConfigSource)> {
        if let Some(path) = explicit {
            let config = Self::from_yaml_with_overrides(path, listen, public_url, overrides)?;
            return Ok((config, ConfigSource::Cli(path.to_path_buf())));
        }
        if let Some(path) = default_path {
            let config = Self::from_yaml_with_overrides(path, listen, public_url, overrides)?;
            return Ok((config, ConfigSource::DefaultPath(path.to_path_buf())));
        }
        let config =
            Self::from_default_yaml_with_overrides(Path::new("."), listen, public_url, overrides)
                .map_err(|err| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("parse bundled config: {err}"),
                    )
                })?;
        Ok((config, ConfigSource::Bundled))
    }

    /// Override-free convenience wrapper used by the test suite's many
    /// parse cases; the binary path always goes through
    /// [`Self::from_yaml_str_with_overrides`].
    #[cfg(test)]
    pub(super) fn from_yaml_str(
        raw: &str,
        base_dir: &Path,
        listen: SocketAddr,
        public_url: Option<String>,
    ) -> Result<Self, RegistryError> {
        Self::from_yaml_str_with_overrides(
            raw,
            base_dir,
            listen,
            public_url,
            FeatureOverrides::default(),
        )
    }

    pub(super) fn from_yaml_str_with_overrides(
        raw: &str,
        base_dir: &Path,
        listen: SocketAddr,
        public_url: Option<String>,
        overrides: FeatureOverrides,
    ) -> Result<Self, RegistryError> {
        let config = Self::from_config_file(
            parse_config_file(raw)?,
            base_dir,
            listen,
            public_url,
            overrides,
        )?;
        config.features.ensure_a_feature_is_enabled()?;
        Ok(config)
    }

    pub(super) fn from_config_file(
        mut file: ConfigFile,
        base_dir: &Path,
        listen: SocketAddr,
        public_url: Option<String>,
        overrides: FeatureOverrides,
    ) -> Result<Self, RegistryError> {
        let hosts_packages = declares_a_hosted_registry(&file) && !overrides.disable_registry;
        let storage = build_storage_config(&mut file, base_dir, hosts_packages)?;
        let http = build_http_config(&mut file, base_dir, listen, public_url)?;
        let backend = super::backend_config::build_backend_config(&storage.metadata);
        let (routing, features) = build_routing_and_features(&mut file, overrides)?;
        let secrets = resolve_secrets(file.secret.as_ref())?;
        Ok(Self {
            logs: build_log_config(file.log.as_ref()),
            osv: build_osv_config(&file.osv, base_dir),
            resolution_cache_secret: Arc::clone(&secrets.cache),
            cache_secret: secrets.cache,
            oci_secret: secrets.oci,
            has_configured_secret: secrets.has_configured,
            http,
            storage,
            identity: build_identity_config(&file.auth, backend)?,
            features,
            routing,
            release_records: super::build_release_records_config(file.release_records.take())?,
            gc: super::build_gc_config(file.gc.take())?,
        })
    }
}

fn build_routing_config(
    resolved: ResolvedFileRegistries,
    routes: Option<RoutesFile>,
) -> Result<super::RoutingConfig, RegistryError> {
    Ok(super::RoutingConfig {
        upstreams: resolved.upstreams,
        route_policy: build_route_policy(routes)?,
        registries: resolved.registries,
        hosted: resolved.hosted,
        tar_registries: resolved.tar_registries,
    })
}

/// Whether the config declares a registry with package metadata of its own.
fn declares_a_hosted_registry(file: &ConfigFile) -> bool {
    file.registries
        .values()
        .any(|registry| matches!(registry, RegistryFile::Hosted(_)))
}

fn build_storage_config(
    file: &mut ConfigFile,
    base_dir: &Path,
    hosts_packages: bool,
) -> Result<super::StorageConfig, RegistryError> {
    let (hosted_dir, cache_dir, cache) = resolve_storage_paths(file, base_dir)?;
    let hosted_backend = file.s3.take().map_or(HostedStoreConfig::Fs, HostedStoreConfig::S3);
    let metadata = crate::storage_config::resolve_metadata_backend(
        file.database.as_ref(),
        &hosted_dir,
        base_dir,
        &hosted_backend,
        hosts_packages,
    )?;
    Ok(super::StorageConfig { hosted_dir, cache_dir, cache, hosted_backend, metadata })
}

pub(super) fn parse_config_file(raw: &str) -> Result<ConfigFile, RegistryError> {
    parse_config_file_with_env::<SystemEnv>(raw)
}

pub(super) fn parse_config_file_with_env<Sys: EnvVar>(
    raw: &str,
) -> Result<ConfigFile, RegistryError> {
    crate::secret::parse_with_env::<Sys, ConfigFile>(raw)
}

fn build_routing_and_features(
    file: &mut ConfigFile,
    overrides: FeatureOverrides,
) -> Result<(super::RoutingConfig, super::Features), RegistryError> {
    let registry_declared = !file.registries.is_empty();
    let resolved = resolve_file_registries(
        std::mem::take(&mut file.registries),
        file.default_registry.take(),
        registry_declared && !overrides.disable_registry,
    )?;
    let routing = build_routing_config(resolved, file.routes.take())?;
    let features = build_features(
        registry_declared,
        file.resolver.take(),
        file.artifacts.take(),
        file.pipeline.take(),
        overrides,
        &routing.hosted,
    )?;
    Ok((routing, features))
}
