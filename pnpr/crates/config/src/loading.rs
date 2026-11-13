use super::{
    Arc, Config, ConfigFile, ConfigSource, DEFAULT_CONFIG_YAML, EnvVar, FeatureOverrides,
    HostedStoreConfig, Management, Path, PathBuf, RegistryError, RoutingConfig, SocketAddr,
    SystemEnv, build_features, build_identity_config, build_log_config, build_osv_config,
    config_file_in, http_config::build_http_config, parse_allowed_private_networks,
    resolve_secrets, resolve_storage_paths,
};
use crate::{
    DocumentOrigin, RegistryGraphDocument, RoutingOptions, config_file::RegistryFile,
    graph_document::is_env_name,
};

impl Config {
    /// Load YAML from `path`. `listen` and `public_url` override the file's
    /// `listen:` and `publicUrl:` keys.
    ///
    /// `storage` from the YAML is resolved relative to the config
    /// file's parent directory when not absolute — same convention
    /// verdaccio uses for `./storage`.
    pub fn from_yaml(
        path: &Path,
        listen: SocketAddr,
        public_url: Option<String>,
    ) -> std::io::Result<Self> {
        Self::from_yaml_with_overrides(path, Some(listen), public_url, FeatureOverrides::default())
    }

    pub(super) fn from_yaml_with_overrides(
        path: &Path,
        listen: Option<SocketAddr>,
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
            Some(listen),
            public_url,
            FeatureOverrides::default(),
        )
        .expect("bundled DEFAULT_CONFIG_YAML must always parse")
    }

    pub(super) fn from_default_yaml_with_overrides(
        base_dir: &Path,
        listen: Option<SocketAddr>,
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
            Some(listen),
            public_url,
            FeatureOverrides::default(),
        )
    }

    /// Like [`Self::resolve`] but applies CLI [`FeatureOverrides`] during
    /// parse, so a surface disabled on the command line skips its parse-time
    /// work (e.g. strict upstream token resolution) — not just its routes.
    /// `listen` is `None` unless the command line sets it, so the file's
    /// `listen:` applies. The binary uses this; tests and embedders that don't
    /// override features call [`Self::resolve`].
    pub fn resolve_with_overrides(
        explicit: Option<&Path>,
        default_path: Option<&Path>,
        listen: Option<SocketAddr>,
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
            Some(listen),
            public_url,
            FeatureOverrides::default(),
        )
    }

    pub(super) fn from_yaml_str_with_overrides(
        raw: &str,
        base_dir: &Path,
        listen: Option<SocketAddr>,
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
        listen: Option<SocketAddr>,
        public_url: Option<String>,
        overrides: FeatureOverrides,
    ) -> Result<Self, RegistryError> {
        let registries_managed_by = file.registries_managed_by;
        let registry_env =
            build_registry_env(registries_managed_by, std::mem::take(&mut file.registry_env))?;
        if registries_managed_by == Management::Api {
            reject_file_graph(&file)?;
        }
        let hosts_packages = (registries_managed_by == Management::Api
            || declares_a_hosted_registry(&file))
            && !overrides.disable_registry;
        let storage = build_storage_config(&mut file, base_dir, hosts_packages)?;
        let http = build_http_config(&mut file, base_dir, listen, public_url)?;
        let backend = super::backend_config::build_backend_config(&storage.metadata);
        let (routing, features) =
            build_routing_and_features(&mut file, registries_managed_by, overrides)?;
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
            registries_managed_by,
            registry_env,
            release_records: super::build_release_records_config(file.release_records.take())?,
            gc: super::build_gc_config(file.gc.take())?,
        })
    }
}

/// `registryEnv`, which only a graph stored in the database reads.
fn build_registry_env(
    managed_by: Management,
    registry_env: Vec<String>,
) -> Result<Vec<String>, RegistryError> {
    if !registry_env.is_empty() && managed_by != Management::Api {
        return Err(RegistryError::InvalidConfig {
            reason: "`registryEnv` lists variables for a registry graph stored in the database; \
                     it requires `registriesManagedBy: api`"
                .to_string(),
        });
    }
    if let Some(name) = registry_env
        .iter()
        .find(|name| !is_env_name(name))
    {
        return Err(RegistryError::InvalidConfig {
            reason: format!("`registryEnv` entry {name:?} is not an environment variable name"),
        });
    }
    Ok(registry_env)
}

/// With `registriesManagedBy: api` the database holds the graph, so the file
/// must not declare one that would be silently ignored.
fn reject_file_graph(file: &ConfigFile) -> Result<(), RegistryError> {
    let declared = [
        ("registries", !file.registries.is_empty()),
        ("defaultRegistry", file.default_registry.is_some()),
        (
            "routes.public",
            file.routes
                .as_ref()
                .is_some_and(|routes| !routes.public.is_empty()),
        ),
    ];
    if let Some((key, _)) = declared.iter().find(|(_, present)| *present) {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "`{key}` is managed through the admin API because of `registriesManagedBy: api`; \
                 import it with `pnpr registries import`",
            ),
        });
    }
    let team_reference = file.artifacts
        .iter()
        .flat_map(|artifacts| artifacts.owners.values())
        .flat_map(|owner| {
            owner.access
                .entries()
                .iter()
                .chain(owner.publish.entries())
        })
        .find(|entry| entry.starts_with("team:"));
    if let Some(entry) = team_reference {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "`artifacts.owners` names {entry:?}, but registry teams live in the database \
                 with `registriesManagedBy: api`; list account IDs instead",
            ),
        });
    }
    Ok(())
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
    managed_by: Management,
    overrides: FeatureOverrides,
) -> Result<(RoutingConfig, super::Features), RegistryError> {
    let registry_declared = managed_by == Management::Api || !file.registries.is_empty();
    let routes = file.routes.take();
    let (public, networks) = routes
        .map(|routes| (routes.public, routes.allowed_private_networks))
        .unwrap_or_default();
    let document = RegistryGraphDocument::from_file_parts(
        std::mem::take(&mut file.registries),
        file.default_registry.take(),
        public,
    );
    let options = RoutingOptions {
        origin: DocumentOrigin::File,
        allowed_private_networks: parse_allowed_private_networks(&networks)?,
        resolve_upstreams: registry_declared && !overrides.disable_registry,
    };
    let routing = RoutingConfig::build_from_document::<SystemEnv>(document, &options)?;
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
