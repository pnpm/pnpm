use super::{
    Arc, Config, ConfigFile, ConfigSource, DEFAULT_CONFIG_YAML, EnvVar, FeatureOverrides,
    HostedStoreConfig, Path, PathBuf, RegistryError, RemovedGroupsBlock, RemovedPackagesBlock,
    RemovedUplinksBlock, RemovedUpstreamsBlock, ResolvedFileRegistries, RoutesFile, SocketAddr,
    SystemEnv, build_backend_config, build_cors_config, build_features, build_identity_config,
    build_log_config, build_osv_config, build_route_policy, build_ui_config, config_file_in,
    env_replace_lossy, resolve_file_registries, resolve_secrets, resolve_storage_paths,
};
use crate::secrets::validate_secret_env_vars;

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
        let storage = build_storage_config(&mut file, base_dir)?;
        let http = build_http_config(&mut file, base_dir, listen, public_url)?;
        let backend = build_backend_config(file.backend, base_dir)?;
        reject_removed_blocks(RemovedBlocks {
            packages: file.packages.as_ref(),
            groups: file.groups.as_ref(),
            uplinks: file.uplinks.as_ref(),
            upstreams: file.upstreams.as_ref(),
        })?;
        let features = build_features(
            !file.registries.is_empty(),
            file.resolver,
            file.artifacts,
            file.pipeline,
            overrides,
        )?;
        let resolved = resolve_file_registries(
            file.registries,
            file.default_registry,
            features.registry.enabled,
        )?;
        let routing = build_routing_config(resolved, file.routes)?;
        let secrets = resolve_secrets(file.secret.as_deref(), file.secrets.as_ref())?;
        Ok(Self {
            logs: build_log_config(file.log.as_ref()),
            osv: build_osv_config(&file.osv, base_dir),
            resolution_cache_secret: Arc::clone(&secrets.cache),
            cache_secret: secrets.cache,
            oci_secret: secrets.oci,
            has_configured_secret: secrets.has_configured,
            http,
            storage,
            identity: build_identity_config(&file.auth, base_dir, backend)?,
            features,
            routing,
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

fn build_http_config(
    file: &mut ConfigFile,
    base_dir: &Path,
    listen: SocketAddr,
    public_url: Option<String>,
) -> Result<super::HttpConfig, RegistryError> {
    let cors = build_cors_config(std::mem::take(&mut file.cors))?;
    let ui = build_ui_config(std::mem::take(&mut file.ui), base_dir);
    let trusted_proxies = super::parse_trusted_proxies(&file.trusted_proxies)?;
    let trusted_proxy_header = match file.trusted_proxy_header.as_deref() {
        Some(raw) => raw.parse::<super::TrustedProxyHeader>()?,
        None => super::TrustedProxyHeader::default(),
    };
    let request_timeout = match file.request_timeout.as_ref() {
        Some(interval) => {
            let duration =
                super::parse_interval(&interval.0)
                    .ok_or_else(|| RegistryError::InvalidConfig {
                        reason: format!(
                            "http.requestTimeout {:?} is not a valid interval",
                            interval.0,
                        ),
                    })?;
            (!duration.is_zero()).then_some(duration)
        }
        None => Some(super::HttpConfig::DEFAULT_REQUEST_TIMEOUT),
    };
    Ok(super::HttpConfig {
        listen,
        public_url: public_url.unwrap_or_else(|| format!("http://{listen}")),
        cors,
        ui,
        oci: std::mem::take(&mut file.oci),
        packument_ttl: match &file.packument_ttl {
            Some(interval) => super::parse_interval(&interval.0)
                .ok_or_else(|| RegistryError::InvalidConfig {
                    reason: format!("packumentTtl {:?} is not a valid interval", interval.0),
                })?,
            None => Config::DEFAULT_PACKUMENT_TTL,
        },
        trusted_proxies,
        trusted_proxy_header,
        request_timeout,
    })
}

fn build_storage_config(
    file: &mut ConfigFile,
    base_dir: &Path,
) -> Result<super::StorageConfig, RegistryError> {
    let (hosted_dir, cache_dir, cache) = resolve_storage_paths(file, base_dir)?;
    Ok(super::StorageConfig {
        hosted_dir,
        cache_dir,
        cache,
        hosted_backend: file.s3.take().map_or(HostedStoreConfig::Fs, HostedStoreConfig::S3),
    })
}

pub(super) fn parse_config_file(raw: &str) -> Result<ConfigFile, RegistryError> {
    parse_config_file_with_env::<SystemEnv>(raw)
}

pub(super) fn parse_config_file_with_env<Sys: EnvVar>(
    raw: &str,
) -> Result<ConfigFile, RegistryError> {
    validate_secret_env_vars::<Sys>(raw)?;
    let (substituted, unresolved) = env_replace_lossy::<Sys>(raw);
    if !unresolved.is_empty() {
        tracing::warn!(?unresolved, "config references unset environment variables");
    }
    let file: ConfigFile = serde_saphyr::from_str(&substituted)
        .map_err(|err| RegistryError::InvalidConfig { reason: err.to_string() })?;
    if file.oci.max_blob_bytes == 0 || file.oci.max_manifest_bytes == 0 {
        return Err(RegistryError::InvalidConfig {
            reason: "oci size limits must be greater than zero".to_string(),
        });
    }
    Ok(file)
}

/// The global ACL and group blocks are gone, not ignorable: they used
/// to *enforce* and *grant* access, so dropping either like an unknown
/// verdaccio key would silently change who may reach what on upgrade.
/// Fail loudly instead, naming the replacement.
/// The top-level blocks a config may still carry from an older release,
/// each one [`Some`] only while the config declares it.
#[derive(Clone, Copy)]
pub(super) struct RemovedBlocks<'a> {
    pub(super) packages: Option<&'a RemovedPackagesBlock>,
    pub(super) groups: Option<&'a RemovedGroupsBlock>,
    pub(super) uplinks: Option<&'a RemovedUplinksBlock>,
    pub(super) upstreams: Option<&'a RemovedUpstreamsBlock>,
}

pub(super) fn reject_removed_blocks(blocks: RemovedBlocks<'_>) -> Result<(), RegistryError> {
    if blocks.packages.is_some() {
        return Err(RegistryError::InvalidConfig {
            reason: "the top-level `packages:` block was removed: declare per-package rules \
                     on the registry that serves them, as `registries.<name>.packages` \
                     (pattern keys, `access`/`publish`/`unpublish` values)"
                .to_string(),
        });
    }
    if blocks.groups.is_some() {
        return Err(RegistryError::InvalidConfig {
            reason: "the top-level `groups:` block was removed: declare teams on the \
                     registry that uses them, as `registries.<name>.teams` (team-name keys, \
                     member-list values), and reference them from that registry's access \
                     lists as `team:<name>`"
                .to_string(),
        });
    }
    if blocks.uplinks.is_some() {
        return Err(RegistryError::InvalidConfig {
            reason: "the top-level `uplinks:` block is not supported: declare upstream \
                     registries under `registries:` with `type: upstream`"
                .to_string(),
        });
    }
    if blocks.upstreams.is_some() {
        return Err(RegistryError::InvalidConfig {
            reason: "the top-level `upstreams:` block was removed: declare upstream \
                     registries under `registries:` with `type: upstream`"
                .to_string(),
        });
    }
    Ok(())
}
