#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

pub mod oidc;

pub use access::{
    AccessSpec, PackageAccess, compile_access_list, validate_member_name, validate_team_name,
};
pub use artifacts::{ArtifactQuota, ArtifactsFeature, StorageAccess};
pub use backend_config::{
    AuthConfig, BackendConfig, HtpasswdConfig, IdentityConfig, LibsqlSettings, MaxUsers,
    SqlBackendSettings, TokensConfig,
};
pub use byte_size::{ByteSize, parse_byte_size};
pub use hosted::{HostedConfig, Management};
pub use ip_network::{IpNetwork, parse_trusted_proxies};
pub use logging::{LogConfig, LogFormat, LogLevel};
pub use pnpr_policy::{TeamDirectory, Teams};
pub use s3::{HostedStoreConfig, S3Settings, build_s3_store, normalize_key_prefix};
pub use scim::ScimConfig;
pub use secrets::{MIN_RESOLUTION_SECRET_LEN, derive_subkey, random_secret, resolution_secret};
pub use storage_config::{CacheConfig, StorageConfig, default_cache_dir};
pub use trusted_proxy_header::TrustedProxyHeader;
pub use upstream::{RedactedHeaders, UpstreamConfig, UpstreamRequestPolicy, parse_interval};
pub use web_ui::UiConfig;

pub(crate) use secrets::resolve_secrets;
pub(crate) use storage_config::{default_storage_string, resolve_relative, resolve_storage_paths};

mod access;
use access::{build_admins, build_teams};

mod artifacts;
use artifacts::{parse_artifact_quota, parse_storage_access};

mod backend_config;
use backend_config::{build_backend_config, build_identity_config};

mod byte_size;

mod config_file;
use config_file::{
    ArtifactQuotaFile, ArtifactsFeatureFile, AuthFile, BackendFile, ConfigFile, CorsFile,
    DefaultRegistryFile, FeatureFile, HostedFile, LibsqlFile, LogEntryFile, OsvFile,
    PipelineFeatureFile, RegistryFile, RegistryGroupFile, RemovedGroupsBlock, RemovedPackagesBlock,
    RemovedUplinksBlock, RemovedUpstreamsBlock, RoutesFile, SqlBackendFile, StorageAccessFile,
    UpstreamFile,
};

mod hosted;
mod ip_network;
mod loading;
mod logging;
use logging::build_log_config;

mod registry_graph;
use registry_graph::{
    ResolvedFileRegistries, org_collision_error, registry_err, resolve_file_registries,
    validate_org_namespace, validate_registry_key, validate_registry_name,
};

mod s3;
mod scim;
mod secrets;
mod storage_config;
mod trusted_proxy_header;

mod upstream;
use upstream::{Interval, UpstreamAuthFile, UpstreamConfigFile, resolve_upstream_config};

mod validation;

mod web_ui;
use web_ui::build_ui_config;

use indexmap::IndexMap;
use object_store::{
    ObjectStore,
    aws::{AmazonS3Builder, AmazonS3ConfigKey},
};
use pnpm_env_replace::{EnvVar, SystemEnv, env_replace_lossy};
use pnpr_error::{RegistryError, redact_url_credentials};
use pnpr_policy::{AccessList, AccessToken, Identity, PackageRule, PackageRules};
use pnpr_registry::{Ecosystem, PackagePattern, Registries, Registry, RegistryConfigError};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    fmt,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

/// The bundled default YAML config. Other crates can
/// pull this in directly when they need pnpr's defaults
/// (upstreams, package routing) without reading a file from disk —
/// e.g. test mocks that want to run with the standard `**` -> `npmjs`
/// routing applied.
pub const DEFAULT_CONFIG_YAML: &str = include_str!("../config.yaml");

/// Where the live [`Config`] came from. Returned alongside
/// [`Config::resolve`] so the binary can log the resolved source
/// (path or "bundled") after the tracing subscriber is up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// User passed `-c` / `--config`.
    Cli(PathBuf),
    /// Loaded from the auto-discovered global config (the `pnpr`
    /// config dir; see [`Config::auto_config_path`]).
    DefaultPath(PathBuf),
    /// No file was found; the bundled [`DEFAULT_CONFIG_YAML`] was
    /// used.
    Bundled,
}

/// Runtime configuration for the pnpm registry server.
///
/// Top-level sections configure storage, auth, routing, and server behavior.
/// Runtime-only fields (`listen`, `public_url`, `packument_ttl`)
/// can be overridden by the binary's CLI flags after the YAML is loaded.
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "server configuration aggregates all functional subsystems"
    )
)]
#[derive(Clone)]
pub struct Config {
    /// Format and level for the `tracing-subscriber` the binary
    /// installs at startup. Sourced from the YAML `log:` object.
    /// Defaults to pretty/info.
    pub logs: LogConfig,
    /// Optional local OSV database used by mounted surfaces to reject
    /// known vulnerable npm package versions without live API calls.
    pub osv: OsvConfig,
    /// Secret keying the HMAC that namespaces private resolution-cache
    /// entries, so the private key is not correlatable offline. Sourced
    /// from the YAML `secret:` key when present; otherwise a fresh
    /// 32-byte value from the OS CSPRNG at startup (private entries then
    /// live only for this process's lifetime).
    pub resolution_cache_secret: Arc<[u8]>,
    /// Secret keying private cache HMACs (resolution cache and upstream cache digest).
    pub cache_secret: Arc<[u8]>,
    /// Secret keying OCI token signing.
    pub oci_secret: Arc<[u8]>,
    /// Whether `secret:` or `secrets:` was explicitly provided in configuration rather than generated randomly.
    pub has_configured_secret: bool,
    pub http: HttpConfig,
    pub storage: StorageConfig,
    pub identity: IdentityConfig,
    pub features: Features,
    pub routing: RoutingConfig,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("logs", &self.logs)
            .field("osv", &self.osv)
            .field("resolution_cache_secret", &"[redacted]")
            .field("cache_secret", &"[redacted]")
            .field("oci_secret", &"[redacted]")
            .field("has_configured_secret", &self.has_configured_secret)
            .field("http", &self.http)
            .field("storage", &self.storage)
            .field("identity", &self.identity)
            .field("features", &self.features)
            .field("routing", &self.routing)
            .finish()
    }
}

#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "registry HTTP configuration reflects top-level server options"
    )
)]
#[derive(Debug, Clone)]
pub struct HttpConfig {
    /// Address the HTTP server binds to.
    pub listen: SocketAddr,
    /// URL clients should use to reach this server. Used to rewrite
    /// `dist.tarball` URLs in served packuments so tarball requests
    /// flow through this server.
    pub public_url: String,
    /// Cross-origin browser access. Empty by default, so pnpr emits no CORS
    /// response headers unless an operator explicitly names trusted origins.
    pub cors: CorsConfig,
    pub ui: UiConfig,
    /// OCI authentication and size limits.
    pub oci: OciConfig,
    /// How long a cached packument is considered fresh before it is
    /// re-fetched from the resolved upstream. Ignored when no upstream
    /// matches.
    pub packument_ttl: Duration,
    /// Reverse-proxy networks trusted to supply client IP headers.
    pub trusted_proxies: Vec<IpNetwork>,
    /// Which forwarding header to inspect when behind a trusted proxy.
    pub trusted_proxy_header: TrustedProxyHeader,
    /// Request processing timeout. If a request handler takes longer than
    /// this duration to process, it is aborted and returns 504 Gateway Timeout.
    /// `None` or 0 disables request timeouts.
    pub request_timeout: Option<Duration>,
}

impl HttpConfig {
    pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_mins(1);
}

#[derive(Debug, Clone)]
pub struct RoutingConfig {
    /// Upstream-registry backends, keyed by registry id. Built from the `registries:`
    /// `upstream` entries and consumed by the `/~<name>/` serving and route
    /// classification.
    pub upstreams: IndexMap<String, UpstreamConfig>,
    /// Which fetch routes the resolution cache treats as public (fetched
    /// anonymously and shared globally) vs. private, driving the
    /// resolver's route classification.
    pub route_policy: RoutePolicy,
    /// The validated registry routing graph: every addressable origin
    /// (`/~<name>/`) plus the optional path-less default target. Concrete
    /// upstream registries are backed by [`Self::upstreams`]; hosted registries by
    /// [`Self::hosted`]; each declares the package-name patterns it serves,
    /// and a router selects the first of its sources whose patterns claim the
    /// name. Built and validated at config load — a misordered or
    /// self-referential router fails startup rather than serving the wrong
    /// origin.
    pub registries: Registries,
    /// Hosted registries, keyed by registry id. Each owns an `org` storage/serving
    /// namespace and an access policy gating its packages. The only registry kind
    /// that accepts writes.
    pub hosted: IndexMap<String, HostedConfig>,
    /// The registries, by key, that declare `tarballs: tar`: their metadata
    /// and tarball routes describe uncompressed tars instead of `.tgz` files.
    pub tar_registries: BTreeSet<String>,
}

/// The format a registry serves tarballs in at its own URL.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TarballFormat {
    /// The published `.tgz`, as npm serves it.
    #[default]
    Tgz,
    /// The gzip-decoded `.tgz`, encoded per request with HTTP content coding.
    Tar,
}

/// Authentication and byte limits for the OCI distribution surface.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct OciConfig {
    #[serde(default, rename = "bearerAuth", alias = "bearer_auth")]
    pub bearer_auth: bool,
    #[serde(default, rename = "maxBlobBytes", alias = "max_blob_bytes")]
    pub max_blob_bytes: u64,
    #[serde(default, rename = "maxManifestBytes", alias = "max_manifest_bytes")]
    pub max_manifest_bytes: usize,
}

impl Default for OciConfig {
    fn default() -> Self {
        Self {
            bearer_auth: false,
            max_blob_bytes: 10 * 1024 * 1024 * 1024,
            max_manifest_bytes: 4 * 1024 * 1024,
        }
    }
}

/// Exact browser origins allowed to call pnpr across origins.
#[derive(Debug, Default, Clone)]
pub struct CorsConfig {
    allowed_origins: Vec<String>,
}

impl CorsConfig {
    pub fn from_allowed_origins(
        origins: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<Self, RegistryError> {
        let mut allowed_origins = Vec::new();
        for origin in origins {
            let origin = normalize_cors_origin(origin.as_ref())?;
            if !allowed_origins.contains(&origin) {
                allowed_origins.push(origin);
            }
        }
        Ok(Self { allowed_origins })
    }

    #[must_use]
    pub fn allowed_origins(&self) -> &[String] {
        &self.allowed_origins
    }
}

/// Which fetch routes the resolution cache treats as public. The official
/// `registry.npmjs.org` host is always a built-in public route (added by the
/// route layer when it builds its classification context); these are the
/// *additional* operator-declared ones.
#[derive(Debug, Default, Clone)]
pub struct RoutePolicy {
    /// Operator-declared public routes, matched by registry prefix
    /// and/or package pattern.
    pub public: Vec<PublicRoute>,
    /// Non-public networks the resolver may still connect to, such as the
    /// one an internal upstream registry sits on. The resolver refuses every
    /// other loopback, private, link-local, and reserved address, except for
    /// this server's own `public_url` host.
    pub allowed_private_networks: Vec<IpNetwork>,
}

/// One operator-declared public route. A fetch matches when its registry
/// URL is under `registry` (when set) and its package name matches
/// `package` (when set); an all-`None` rule matches every fetch.
#[derive(Debug, Clone)]
pub struct PublicRoute {
    pub registry: Option<String>,
    pub package: Option<String>,
}

/// State of the npm-registry surface. There is no YAML toggle for it:
/// the surface is served iff at least one registry is declared under
/// `registries:` (no registries ⇒ nothing to serve), minus the per-tier
/// `--disable-registry` CLI override. A dedicated type — rather than a
/// bare `bool` on [`Config`] — so finer-grained registry sub-features
/// (e.g. disabling `publish` for a read-only mirror) can be added here
/// without changing the config shape.
#[derive(Debug, Clone)]
pub struct RegistryFeature {
    /// Master switch for the whole npm-registry surface. When `false`,
    /// none of the registry routes are mounted.
    pub enabled: bool,
}

impl Default for RegistryFeature {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Toggle for the install-accelerator (resolver) surface. Separate from
/// [`RegistryFeature`] so each surface grows its own sub-features
/// independently.
#[derive(Debug, Clone)]
pub struct ResolverFeature {
    /// Master switch for the resolver surface (`/-/pnpr`, `/-/pnpr/v0/resolve`,
    /// `/-/pnpr/v0/verify-lockfile`). When `false`, none of those routes are
    /// mounted.
    pub enabled: bool,
}

impl Default for ResolverFeature {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Toggle for the pipeline run-record surface (`/-/pnpr/v0/pipeline*`).
/// Off by default while the surface is a proof of concept. Who may read and
/// publish runs is [`ArtifactsFeature::orgs`].
#[derive(Debug, Default, Clone)]
pub struct PipelineFeature {
    /// Master switch for the run submission, listing, and viewer endpoints.
    pub enabled: bool,
}

/// CLI-level overrides for the feature toggles, applied *during* config
/// parse so the effective surface enablement is known before any
/// registry-only work runs. This matters because upstream resolution is
/// strict (a `upstream.auth` block with an unresolvable token is a config
/// error): applying `--disable-registry` only after parsing would still
/// force a resolver-only tier to carry upstream secrets. A `true` field
/// forces the corresponding surface off regardless of what the config
/// file declares.
#[derive(Debug, Default, Clone, Copy)]
pub struct FeatureOverrides {
    pub disable_registry: bool,
    pub disable_resolver: bool,
    pub disable_artifacts: bool,
    pub disable_pipeline: bool,
}

#[derive(Debug, Default, Clone)]
pub struct OsvConfig {
    pub enabled: bool,
    pub path: Option<PathBuf>,
}

impl Config {
    /// Default `listen` when one isn't supplied by the caller.
    pub const DEFAULT_LISTEN: &'static str = "127.0.0.1:7677";
    /// Default packument TTL — five minutes.
    pub const DEFAULT_PACKUMENT_TTL: Duration = Duration::from_mins(5);
}

/// The feature toggles the file declares, with the CLI overrides folded in.
#[derive(Debug, Clone)]
pub struct Features {
    pub registry: RegistryFeature,
    pub resolver: ResolverFeature,
    pub artifacts: ArtifactsFeature,
    pub pipeline: PipelineFeature,
}

/// Folding the `--disable-registry` override in here lets the registry-only
/// work below (upstream credential resolution) key off effective enablement.
fn build_features(
    registry_declared: bool,
    resolver: Option<FeatureFile>,
    artifacts: Option<ArtifactsFeatureFile>,
    pipeline: Option<PipelineFeatureFile>,
    overrides: FeatureOverrides,
) -> Result<Features, RegistryError> {
    let resolver_file = resolver.unwrap_or_default();
    let artifacts_file = artifacts.unwrap_or_default();
    let pipeline_file = pipeline.unwrap_or_default();
    Ok(Features {
        registry: RegistryFeature { enabled: registry_declared && !overrides.disable_registry },
        resolver: ResolverFeature { enabled: resolver_file.enabled && !overrides.disable_resolver },
        artifacts: ArtifactsFeature {
            enabled: artifacts_file.enabled && !overrides.disable_artifacts,
            orgs: parse_storage_access(artifacts_file.orgs)?,
            quota: parse_artifact_quota(&artifacts_file.quota)?,
        },
        pipeline: PipelineFeature { enabled: pipeline_file.enabled && !overrides.disable_pipeline },
    })
}

fn build_cors_config(file: CorsFile) -> Result<CorsConfig, RegistryError> {
    CorsConfig::from_allowed_origins(file.allowed_origins)
}

fn normalize_cors_origin(raw: &str) -> Result<String, RegistryError> {
    let parsed = url::Url::parse(raw)
        .map_err(|_| RegistryError::InvalidConfig {
            reason: format!("CORS allowed origin {raw:?} is not an absolute URL"),
        })?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
    {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "CORS allowed origin {raw:?} must contain only an http(s) scheme, host, and optional port",
            ),
        });
    }
    Ok(parsed.origin().ascii_serialization())
}

fn build_route_policy(file: Option<RoutesFile>) -> Result<RoutePolicy, RegistryError> {
    let Some(file) = file else { return Ok(RoutePolicy::default()) };
    Ok(RoutePolicy {
        public: file.public
            .into_iter()
            .map(|route| PublicRoute { registry: route.registry, package: route.package })
            .collect(),
        allowed_private_networks: file.allowed_private_networks
            .iter()
            .map(|network| IpNetwork::parse(network))
            .collect::<Result<_, _>>()?,
    })
}

fn build_osv_config(file: &OsvFile, base_dir: &Path) -> OsvConfig {
    OsvConfig {
        enabled: file.enabled,
        path: file.path.as_deref().map(|path| resolve_relative(path, base_dir)),
    }
}

/// Join `config.yaml` onto a resolved config directory and keep the
/// path only when it points at an existing file (so a directory or a
/// missing entry falls back to the bundled config).
fn config_file_in(dir: Option<PathBuf>) -> Option<PathBuf> {
    let path = dir?.join("config.yaml");
    path.is_file().then_some(path)
}

/// `tokens.db` next to the htpasswd file. The sibling layout lets an
/// operator lock the auth directory down with a single chmod and
/// stops the tokens file from leaking into a `storage` directory
/// that may be served over HTTP through an unrelated misconfig.
fn default_tokens_path_sibling_of(htpasswd: &Path) -> PathBuf {
    htpasswd
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("tokens.db")
}

#[cfg(test)]
mod tests;
