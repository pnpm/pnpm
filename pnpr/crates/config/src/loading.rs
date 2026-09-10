use super::{
    Config, ConfigFile, ConfigSource, DEFAULT_CONFIG_YAML, FeatureOverrides, HostedStoreConfig,
    Path, PathBuf, RegistryError, ResolvedFileRegistries, SocketAddr, build_auth_config,
    build_backend_config, build_cors_config, build_features, build_log_config, build_osv_config,
    build_route_policy, config_file_in, parse_config_file, reject_removed_blocks,
    resolution_secret, resolve_file_registries, resolve_storage_paths,
};

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
        let raw = std::fs::read_to_string(path).map_err(|err| {
            std::io::Error::new(err.kind(), format!("read {}: {err}", path.display()))
        })?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        Self::from_yaml_str_with_overrides(&raw, base, listen, public_url, overrides).map_err(
            |err| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("parse {}: {err}", path.display()),
                )
            },
        )
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
        config.ensure_a_feature_is_enabled()?;
        Ok(config)
    }

    pub(super) fn from_config_file(
        file: ConfigFile,
        base_dir: &Path,
        listen: SocketAddr,
        public_url: Option<String>,
        overrides: FeatureOverrides,
    ) -> Result<Self, RegistryError> {
        let (storage, cache_storage) = resolve_storage_paths(&file, base_dir);
        let backend = build_backend_config(file.backend, base_dir)?;
        let cors = build_cors_config(file.cors)?;
        reject_removed_blocks(file.packages.is_some(), file.groups.is_some())?;
        let features = build_features(
            !file.registries.is_empty(),
            file.resolver,
            file.artifacts,
            file.pipeline,
            overrides,
        )?;
        let ResolvedFileRegistries { upstreams, hosted, registries } = resolve_file_registries(
            file.registries,
            file.default_registry,
            features.registry.enabled,
        )?;
        Ok(Self {
            listen,
            public_url: public_url.unwrap_or_else(|| format!("http://{listen}")),
            cors,
            oci: file.oci,
            storage,
            cache_storage,
            upstreams,
            packument_ttl: Self::DEFAULT_PACKUMENT_TTL,
            auth: build_auth_config(&file.auth, base_dir),
            logs: build_log_config(file.log.as_ref()),
            hosted_store: file.s3.map_or(HostedStoreConfig::Fs, HostedStoreConfig::S3),
            backend,
            osv: build_osv_config(&file.osv, base_dir),
            registry: features.registry,
            resolver: features.resolver,
            artifacts: features.artifacts,
            pipeline: features.pipeline,
            route_policy: build_route_policy(file.routes),
            resolution_cache_secret: resolution_secret(file.secret.as_deref())?,
            registries,
            hosted,
        })
    }
}
