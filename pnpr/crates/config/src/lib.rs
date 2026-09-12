pub mod oidc;

pub use logging::{LogConfig, LogFormat, LogLevel};

pub use backend_config::{
    AuthConfig, BackendConfig, HtpasswdConfig, LibsqlSettings, MaxUsers, SqlBackendSettings,
    TokensConfig,
};

pub use access::{AccessSpec, PackageAccess, Teams};

pub use s3::{HostedStoreConfig, S3Settings, build_s3_store, normalize_key_prefix};

pub use self::upstream::{RedactedHeaders, UpstreamConfig};

mod logging;
use logging::build_log_config;

mod backend_config;
use backend_config::{build_auth_config, build_backend_config};

mod loading;

mod validation;

mod presets;

mod config_file;
use config_file::{
    ArtifactsFeatureFile, AuthFile, BackendFile, ConfigFile, CorsFile, DefaultRegistryFile,
    FeatureFile, HostedFile, LogEntryFile, OsvFile, PipelineFeatureFile, RegistryFile,
    RegistryGroupFile, RoutesFile, SqlBackendFile, StorageAccessFile, UpstreamFile,
    parse_config_file, reject_removed_blocks,
};

mod registry_graph;
use registry_graph::{
    ResolvedFileRegistries, org_collision_error, registry_err, registry_mock_graph,
    resolve_file_registries, validate_org_namespace, validate_registry_key, validate_registry_name,
};

mod access;
use access::build_teams;

mod s3;

mod upstream;

use self::upstream::{
    Interval, UpstreamAuthFile, UpstreamConfigFile, parse_interval, resolve_upstream_config,
};

use indexmap::IndexMap;
use object_store::{
    ObjectStore,
    aws::{AmazonS3Builder, AmazonS3ConfigKey},
};
use pnpm_env_replace::{EnvVar, SystemEnv, env_replace_lossy};
use pnpr_error::{RegistryError, redact_url_credentials};
use pnpr_policy::{AccessList, AccessToken, PackageRule, PackageRules};
use pnpr_registry::{Ecosystem, PackagePattern, Registries, Registry, RegistryConfigError};
use reqwest::header::HeaderMap;
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    fmt,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

/// The bundled verdaccio-shaped YAML config, mirrored from
/// `@pnpm/registry-mock`'s `registry/config.yaml`. Other crates can
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
/// The persisted (YAML) shape follows verdaccio's `config.yaml` —
/// `storage`, `upstreams`, `packages` — restricted to the subset
/// pnpr implements (no web UI, auth, plugins, or logs
/// routing).
///
/// Runtime-only fields (`listen`, `public_url`, `packument_ttl`)
/// are set by the binary's CLI flags after the YAML is loaded,
/// matching verdaccio's CLI overrides.
#[derive(Debug, Clone)]
pub struct Config {
    /// Address the HTTP server binds to.
    pub listen: SocketAddr,
    /// URL clients should use to reach this server. Used to rewrite
    /// `dist.tarball` URLs in served packuments so tarball requests
    /// flow through this server.
    pub public_url: String,
    /// Cross-origin browser access. Empty by default, so pnpr emits no CORS
    /// response headers unless an operator explicitly names trusted origins.
    pub cors: CorsConfig,
    /// OCI authentication and size limits.
    pub oci: OciConfig,
    /// Directory under which authoritative packuments and tarballs
    /// live: packages published to this server and the content served
    /// in static mode. This is the source of truth — it is never
    /// overwritten by an upstream refresh, so operators back it up and
    /// keep it on a durable volume.
    pub storage: PathBuf,
    /// Directory under which the disposable proxy cache lives —
    /// the mirror of upstream registries plus the resolver's cache.
    /// Safe to wipe at any time; it self-heals on the next
    /// request. Defaults to a `.pnpr-cache` subdirectory of
    /// [`Self::storage`]; set the YAML `cache:` key (or `--cache`) to
    /// an absolute path to put it on separate, ephemeral disk.
    pub cache_storage: PathBuf,
    /// Upstream-registry backends, keyed by registry id. Built from the `registries:`
    /// `upstream` entries and consumed by the `/~<name>/` serving and route
    /// classification.
    pub upstreams: IndexMap<String, UpstreamConfig>,
    /// How long a cached packument is considered fresh before it is
    /// re-fetched from the resolved upstream. Ignored when no upstream
    /// matches.
    pub packument_ttl: Duration,
    /// Where to read/write the htpasswd-format user file and the
    /// token database. Both stores are in-memory when their paths
    /// are `None`, matching the original `@pnpm/registry-mock` mode
    /// where every restart wipes accounts.
    pub auth: AuthConfig,
    /// Format and level for the `tracing-subscriber` the binary
    /// installs at startup. Sourced from the YAML `log:` object
    /// (Verdaccio 6+ shape). Defaults to pretty/info.
    pub logs: LogConfig,
    /// Where the authoritative (hosted) store lives. Defaults to
    /// [`HostedStoreConfig::Fs`] — the local [`Self::storage`]
    /// directory. The YAML `s3:` block switches it to an S3-compatible
    /// object store (S3, Cloudflare R2, `MinIO`, ...).
    pub hosted_store: HostedStoreConfig,
    /// Which record store backs the auth state (users + tokens).
    /// Defaults to [`BackendConfig::Local`] — today's htpasswd file
    /// plus `SQLite` token database. The YAML `backend:` block can
    /// switch both stores to one shared SQL database so several
    /// stateless pnpr replicas see a consistent set of accounts.
    pub backend: BackendConfig,
    /// Optional local OSV database used by mounted surfaces to reject
    /// known vulnerable npm package versions without live API calls.
    pub osv: OsvConfig,
    /// The npm-registry surface: packument and tarball reads, publish,
    /// unpublish, dist-tag, and search. Derived, not configured: the
    /// surface is served iff at least one registry is declared, minus the
    /// `--disable-registry` per-tier override (a stateless resolver tier
    /// in front of an existing registry). See [`RegistryFeature`].
    pub registry: RegistryFeature,
    /// The install-accelerator surface: the `/-/pnpr` handshake and the
    /// `/-/pnpr/v0/resolve` / `/-/pnpr/v0/verify-lockfile` endpoints. Enabled by
    /// default; disable it to run a plain registry with no server-side
    /// resolution. See [`ResolverFeature`].
    pub resolver: ResolverFeature,
    /// Signed build artifacts and compiler caches. Kept separate from the
    /// resolver so deployments can scale the compute-bound resolver and the
    /// I/O-bound artifact store independently. See [`ArtifactsFeature`].
    pub artifacts: ArtifactsFeature,
    /// The pipeline run-record surface. See [`PipelineFeature`].
    pub pipeline: PipelineFeature,
    /// Which fetch routes the resolution cache treats as public (fetched
    /// anonymously and shared globally) vs. private, driving the
    /// resolver's route classification.
    pub route_policy: RoutePolicy,
    /// Secret keying the HMAC that namespaces private resolution-cache
    /// entries, so the private key is not correlatable offline. Sourced
    /// from the YAML `secret:` key when present; otherwise a fresh
    /// 32-byte value from the OS CSPRNG at startup (private entries then
    /// live only for this process's lifetime).
    pub resolution_cache_secret: Arc<[u8]>,
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
}

/// Authentication and byte limits for the OCI distribution surface.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct OciConfig {
    pub bearer_auth: bool,
    pub max_blob_bytes: u64,
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

/// A resolved hosted registry: the `org` namespace it serves and its
/// `packages:` rules — the namespace it claims plus the per-package
/// `access` / `publish` / `unpublish` policies, with the registry-level
/// `access:` as the default an entry's omitted fields fall back to.
#[derive(Debug, Clone)]
pub struct HostedConfig {
    /// The storage/serving namespace, so two hosted registries holding the same
    /// `name@version` never collide. Empty (`""`) ⇒ the flat `storage` root.
    pub org: String,
    /// The registry's `packages:` map: namespace and per-package rules in one
    /// declaration, selected by specificity. The effective `access` gates
    /// reads *and* the write routing (publish, dist-tag, unpublish), with a
    /// denied caller masked as not-found either way.
    pub rules: PackageRules,
    /// The registry's declared `teams:` map, retained so the npm team API
    /// (`GET /-/org/{scope}/team`, `GET /-/team/{scope}/{team}/user`) can
    /// list them. Membership is config-declared: the API serves reads only,
    /// and team mutations are rejected.
    pub teams: Teams,
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

/// Toggle for the shared-artifact surface. Off by default while the
/// protocol is a proof of concept.
#[derive(Debug, Default, Clone)]
pub struct ArtifactsFeature {
    /// Master switch for artifact and compiler-cache endpoints.
    pub enabled: bool,
    /// Named compiler caches with independent read and publication policies.
    pub compiler_caches: IndexMap<String, StorageAccess>,
}

#[derive(Debug, Clone)]
pub struct StorageAccess {
    pub access: AccessList,
    pub publish: AccessList,
}

/// Toggle for the pipeline run-record surface (`/-/pnpr/v0/pipeline*`).
/// Off by default while the surface is a proof of concept.
#[derive(Debug, Default, Clone)]
pub struct PipelineFeature {
    /// Master switch for the run submission, listing, and viewer endpoints.
    pub enabled: bool,
    pub workspaces: IndexMap<String, StorageAccess>,
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
}

#[derive(Debug, Default, Clone)]
pub struct OsvConfig {
    pub enabled: bool,
    pub path: Option<PathBuf>,
}

/// The namespace [`Config::proxy`] declares on its flat-root hosted org, so
/// those names resolve locally rather than to the npm upstream: the
/// registry-mock fixture scopes plus the unscoped fixtures. Kept in sync
/// with the fixtures under `pnpr/.fixtures/packages` and with the
/// fixture-scope subset of the bundled `config.yaml` `local` registry — the
/// YAML additionally claims the exact names the TS test suite publishes,
/// which pacquet's in-process registry never sees. The fixture packages
/// living in real, active npm scopes (`@pnpm`, `@zkochan`) are claimed by
/// exact name so the rest of those scopes keeps proxying npm (dependency
/// trees of proxied packages pull real `@pnpm/*` packages).
const REGISTRY_MOCK_LOCAL_PATTERNS: &[&str] = &[
    "@foo/*",
    "@having/*",
    "@jsr/*",
    "@pnpm.e2e/*",
    "@private/*",
    "@scoped/*",
    "@pnpm/plugin-pnpmfile",
    "@pnpm/postinstall-modifies-source",
    "@pnpm/x",
    "@pnpm/xyz",
    "@pnpm/xyz-parent",
    "@pnpm/xyz-parent-parent",
    "@pnpm/xyz-parent-parent-parent",
    "@pnpm/xyz-parent-parent-parent-parent",
    "@pnpm/xyz-parent-parent-with-xyz",
    "@pnpm/y",
    "@pnpm/z",
    "@zkochan/test-pnpm-issue219",
    "ajv",
    "ajv-keywords",
    "create-touch-file-one-bin",
];

/// The `local` hosted registry's `packages:` rules in the registry-mock
/// shape: the fixture namespace ([`REGISTRY_MOCK_LOCAL_PATTERNS`]) with
/// default rules, `@private/*` and `@pnpm.e2e/needs-auth` restricted to
/// authenticated callers (the rules `@pnpm/registry-mock` applied under
/// verdaccio), and unpublish open to any authenticated user so the
/// fixture-rewriting test flows keep working. The exact
/// `@pnpm.e2e/needs-auth` key wins over the `@pnpm.e2e/*` scope key by
/// specificity.
fn registry_mock_rules() -> PackageRules {
    let authenticated = || Some(AccessList::from_tokens(["$authenticated"]));
    let mut rules: Vec<PackageRule> = REGISTRY_MOCK_LOCAL_PATTERNS
        .iter()
        .map(|pattern| PackageRule {
            pattern: PackagePattern::parse(pattern, Ecosystem::Npm)
                .expect("valid built-in fixture registry pattern"),
            access: (*pattern == "@private/*").then(authenticated).flatten(),
            publish: (*pattern == "@private/*").then(authenticated).flatten(),
            unpublish: (*pattern == "@private/*").then(authenticated).flatten(),
        })
        .collect();
    rules.push(PackageRule {
        pattern: PackagePattern::parse("@pnpm.e2e/needs-auth", Ecosystem::Npm)
            .expect("valid built-in fixture registry pattern"),
        access: authenticated(),
        publish: authenticated(),
        unpublish: authenticated(),
    });
    PackageRules::new(rules, None)
        .with_default_unpublish(AccessList::from_tokens(["$authenticated"]))
}

impl Config {
    /// Default `listen` when one isn't supplied by the caller.
    pub const DEFAULT_LISTEN: &'static str = "127.0.0.1:7677";
    /// Default packument TTL — five minutes, matching the historical
    /// proxy-mode default.
    pub const DEFAULT_PACKUMENT_TTL: Duration = Duration::from_mins(5);
}

/// The feature toggles the file declares, with the CLI overrides folded in.
struct Features {
    registry: RegistryFeature,
    resolver: ResolverFeature,
    artifacts: ArtifactsFeature,
    pipeline: PipelineFeature,
}

/// The npm-registry surface is derived, not configured: served iff
/// at least one registry is declared (no registries ⇒ nothing to serve),
/// minus the per-tier `--disable-registry` override. Folding the
/// override in here lets the registry-only work below (upstream
/// credential resolution) key off effective enablement.
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
            compiler_caches: parse_storage_access(artifacts_file.compiler_caches)?,
        },
        pipeline: PipelineFeature {
            enabled: pipeline_file.enabled,
            workspaces: parse_storage_access(pipeline_file.workspaces)?,
        },
    })
}

fn build_cors_config(file: CorsFile) -> Result<CorsConfig, RegistryError> {
    CorsConfig::from_allowed_origins(file.allowed_origins)
}

fn normalize_cors_origin(raw: &str) -> Result<String, RegistryError> {
    let parsed = url::Url::parse(raw).map_err(|_| RegistryError::InvalidConfig {
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

fn build_route_policy(file: Option<RoutesFile>) -> RoutePolicy {
    match file {
        None => RoutePolicy::default(),
        Some(file) => RoutePolicy {
            public: file
                .public
                .into_iter()
                .map(|route| PublicRoute { registry: route.registry, package: route.package })
                .collect(),
        },
    }
}

/// Minimum length for an operator-configured `secret:`. A shorter value makes
/// the private-cache descriptor HMAC guessable, defeating its "not
/// correlatable offline" property; a generated secret is 32 bytes.
const MIN_RESOLUTION_SECRET_LEN: usize = 16;

/// The HMAC secret keying private resolution-cache entries: the YAML
/// `secret:` when set (rejected if too short to be a safe HMAC key), else a
/// fresh per-process value.
fn resolution_secret(secret: Option<&str>) -> Result<Arc<[u8]>, RegistryError> {
    match secret {
        Some(secret) if !secret.is_empty() => {
            if secret.len() < MIN_RESOLUTION_SECRET_LEN {
                return Err(RegistryError::InvalidConfig {
                    reason: format!(
                        "`secret:` must be at least {MIN_RESOLUTION_SECRET_LEN} bytes to key the \
                         private resolution-cache HMAC (it is {})",
                        secret.len(),
                    ),
                });
            }
            Ok(Arc::from(secret.as_bytes().to_vec()))
        }
        _ => Ok(random_secret()),
    }
}

/// 32 bytes from the OS CSPRNG, for a deployment that configures no
/// `secret:`. Private cache entries then live only for this process.
fn random_secret() -> Arc<[u8]> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("OS CSPRNG must be available");
    Arc::from(bytes.to_vec())
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
    htpasswd.parent().unwrap_or_else(|| Path::new(".")).join("tokens.db")
}

/// Resolve a (possibly relative) storage path against `base_dir`.
/// Verdaccio's `./storage` convention.
fn resolve_relative(raw: &str, base_dir: &Path) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        return path;
    }
    base_dir.join(path)
}

fn default_storage_string() -> String {
    "./storage".to_string()
}

/// The disposable proxy cache lives under a hidden `.pnpr-cache`
/// subdirectory of `storage` by default. Nesting it under `storage`
/// keeps a `--storage`-only deployment self-contained, while the dot
/// prefix keeps the local search scan (which walks `<storage>/<pkg>`)
/// from treating it as a package. Operators who want the cache on a
/// separate, wipe-able volume point the `cache:` key at an absolute
/// path instead.
#[must_use]
pub fn default_cache_dir(storage: &Path) -> PathBuf {
    storage.join(".pnpr-cache")
}

#[cfg(test)]
mod tests;

fn parse_storage_access(
    policies: IndexMap<String, StorageAccessFile>,
) -> Result<IndexMap<String, StorageAccess>, RegistryError> {
    policies
        .into_iter()
        .map(|(name, policy)| {
            validate_registry_name(&name)?;
            let parse = |spec: &AccessSpec| {
                spec.to_access_list(&Teams::default()).map_err(|reason| {
                    RegistryError::InvalidConfig {
                        reason: format!("storage namespace {name:?}: {reason}"),
                    }
                })
            };
            let access =
                StorageAccess { access: parse(&policy.access)?, publish: parse(&policy.publish)? };
            Ok((name, access))
        })
        .collect()
}

fn resolve_storage_paths(file: &ConfigFile, base_dir: &Path) -> (PathBuf, PathBuf) {
    let storage = resolve_relative(&file.storage, base_dir);
    let cache = file
        .cache
        .as_deref()
        .map_or_else(|| default_cache_dir(&storage), |raw| resolve_relative(raw, base_dir));
    (storage, cache)
}
