use crate::{
    error::RegistryError,
    policy::{AccessGroups, AccessList, Identity, PackageRule, PackageRules},
    registry::{PackagePattern, Registries, Registry, RegistryConfigError},
    s3::{S3Settings, build_s3_store},
};
use indexmap::IndexMap;
use object_store::ObjectStore;
use pacquet_env_replace::{EnvVar, SystemEnv, env_replace_lossy};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use std::{
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
    /// Optional static group memberships used by named access tokens in
    /// package policies and upstream aliases.
    pub groups: AccessGroups,
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
}

#[derive(Debug, Default, Clone)]
pub struct OsvConfig {
    pub enabled: bool,
    pub path: Option<PathBuf>,
}

/// The resolved hosted-store backend. The object-store client is built
/// once at config-load time (the fallible step), so constructing the
/// storage layer from it is infallible.
#[derive(Debug, Clone)]
pub enum HostedStoreConfig {
    /// Local directory — [`Config::storage`].
    Fs,
    /// S3-compatible bucket. `prefix` is normalized to `""` or a
    /// `.../`-terminated key prefix.
    S3 { store: Arc<dyn ObjectStore>, prefix: String },
}

/// The resolved record-store backend for auth (users + tokens). Unlike
/// [`HostedStoreConfig`], this only carries the parsed settings — the
/// fallible step (connecting to the database and ensuring its schema)
/// is async, so it runs in `AuthState::load` rather than at
/// config-parse time.
#[derive(Debug, Default, Clone)]
pub enum BackendConfig {
    /// Local htpasswd users + `SQLite` tokens (or in-memory when no file
    /// is configured). Today's behaviour.
    #[default]
    Local,
    /// Networked `SQLite` (libsql / Turso): both records live in one
    /// shared database reachable over the network.
    Libsql(LibsqlSettings),
    /// `PostgreSQL`: both records live in one shared database.
    Postgres(SqlBackendSettings),
    /// `MySQL`-compatible database: both records live in one shared database.
    Mysql(SqlBackendSettings),
}

/// The YAML `backend.libsql:` block. Whole-file `${ENV}` substitution
/// runs before parsing, so `url`/`authToken` can hold `${...}` refs and
/// keep secrets out of the committed config.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibsqlSettings {
    /// libsql/Turso database URL, e.g. `libsql://db.turso.io` or
    /// `http://127.0.0.1:8080` for a local `sqld`.
    pub url: String,
    /// Bearer token for the database. Omit for an unauthenticated local
    /// `sqld`.
    #[serde(default)]
    pub auth_token: Option<String>,
    /// Local path for an embedded replica. When set, the primary is
    /// replicated to this file and reads (the auth hot path) hit the
    /// local copy instead of a network round-trip per lookup; writes
    /// still go to the primary. Absent ⇒ every read is a remote query.
    #[serde(default)]
    pub replica_path: Option<PathBuf>,
    /// How often (seconds) the embedded replica pulls from the primary.
    /// Only meaningful with `replicaPath`; bounds how stale a read can
    /// be — most importantly, token-revocation lag. `0` disables
    /// background sync (the replica then only reflects its own writes
    /// plus the initial sync at startup). Defaults to
    /// [`LibsqlSettings::DEFAULT_SYNC_INTERVAL_SECS`].
    #[serde(default)]
    pub sync_interval_secs: Option<u64>,
}

impl LibsqlSettings {
    /// Default embedded-replica background sync cadence.
    pub const DEFAULT_SYNC_INTERVAL_SECS: u64 = 60;
}

/// The YAML `backend.postgres:` and `backend.mysql:` blocks. Whole-file
/// `${ENV}` substitution runs before parsing, so `url` can hold
/// `${...}` refs and keep credentials out of the committed config.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlBackendSettings {
    /// Driver connection URL, e.g. `postgres://user:pass@host/db` or
    /// `mysql://user:pass@host/db`.
    pub url: String,
    /// Maximum connections in the backend pool. Defaults to the
    /// driver's pool default when omitted.
    pub max_connections: Option<u32>,
    /// Deadline for request-path auth database operations.
    pub timeout: Duration,
    /// Deadline for initial auth database connect and schema setup.
    pub startup_timeout: Duration,
}

impl SqlBackendSettings {
    /// Default request-path auth database deadline.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
    /// Default startup auth database deadline.
    pub const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_mins(5);
}

/// Auth-related runtime configuration. Built from the YAML
/// `auth:` block plus runtime defaults.
#[derive(Debug, Default, Clone)]
pub struct AuthConfig {
    pub htpasswd: HtpasswdConfig,
    pub tokens: TokensConfig,
}

/// Where the htpasswd users file lives and how many users may sign
/// up before registration is refused.
#[derive(Debug, Default, Clone)]
pub struct HtpasswdConfig {
    /// Absolute path to the htpasswd file. `None` keeps user state
    /// in memory (back-compat with `@pnpm/registry-mock`).
    pub file: Option<PathBuf>,
    /// Cap on new user registrations.
    pub max_users: MaxUsers,
}

/// Where the token database lives. SQLite-backed when `file` is
/// set; an in-memory map otherwise.
#[derive(Debug, Default, Clone)]
pub struct TokensConfig {
    pub file: Option<PathBuf>,
}

/// Three-state cap on `auth.htpasswd.max_users`:
///
/// * absent → registration disabled. Self-registration is opt-in:
///   leaving the key out denies new sign-ups. Verdaccio defaults this
///   to `+infinity`, but an open default lets any anonymous client
///   create an account and then publish under an `$authenticated`
///   policy, so pnpr refuses registration until an operator sets an
///   explicit positive cap.
/// * `-1` → registration disabled
/// * non-negative `n` → at most `n` users
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum MaxUsers {
    #[default]
    Disabled,
    Unlimited,
    Limited(u64),
}

impl MaxUsers {
    /// Translate an explicit YAML value into [`MaxUsers`]. Verdaccio
    /// accepts any signed integer here; negative anything other than
    /// `-1` is nonsense and is treated as "disabled" to err on the
    /// side of rejecting unsafe configs. An omitted key never reaches
    /// this function — it maps to [`MaxUsers::Disabled`] in
    /// [`build_auth_config`], so there is no YAML spelling for
    /// "unlimited".
    fn from_yaml(value: i64) -> Self {
        if value < 0 { MaxUsers::Disabled } else { MaxUsers::Limited(value as u64) }
    }
}

/// Runtime logging configuration. Mirrors the YAML `log:` object
/// (Verdaccio 6+ shape). Drives the `tracing-subscriber` init in the
/// binary: format selects human-readable vs NDJSON, level seeds the
/// default `EnvFilter`.
///
/// Only `type: stdout` is honored — file and syslog sinks are future
/// work. An unsupported `type:` still parses (so a verdaccio config
/// can be copied in untouched) but is ignored at runtime, with a
/// warning logged once the subscriber is up.
#[derive(Debug, Clone)]
pub struct LogConfig {
    pub format: LogFormat,
    pub level: LogLevel,
    /// The configured sink (`log.type`). Only [`Self::STDOUT_SINK`]
    /// is implemented; any other value is recorded here so the binary
    /// can warn about it at startup.
    pub sink: String,
}

impl LogConfig {
    /// The single sink pnpr actually writes to.
    pub const STDOUT_SINK: &'static str = "stdout";

    /// Whether the configured sink is one the server implements.
    #[must_use]
    pub fn sink_is_supported(&self) -> bool {
        self.sink == Self::STDOUT_SINK
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            format: LogFormat::Pretty,
            level: LogLevel::default(),
            sink: Self::STDOUT_SINK.to_string(),
        }
    }
}

/// Wire format for log records. `Pretty` is human-readable with
/// colors when stdout is a TTY; `Json` is NDJSON (one JSON object
/// per record) suitable for log shippers — the same shape pino
/// emits.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Pretty,
    Json,
}

/// Severity threshold. Maps onto `tracing::Level` plus a synthetic
/// `Http` mid-tier (between info and debug) that mirrors what
/// verdaccio and pino call "the request-log level."
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Http,
    #[default]
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// Convert to an `EnvFilter` directive string. `http` is not a
    /// tracing level, so we expand it to `info` for the framework
    /// plus a `pnpr::access=info` target so the per-request
    /// access log surfaces even when the rest of the crate is
    /// quieter.
    #[must_use]
    pub fn as_filter_directive(self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Http => "info,pnpr::access=info",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }
}

/// Runtime upstream declaration: the upstream `url`, the request headers
/// pnpr attaches to every fetch it makes to that upstream, and the
/// verdaccio per-upstream tuning knobs (`maxage`, `timeout`, `max_fails`,
/// `fail_timeout`, `cache`).
///
/// [`Self::headers`] is resolved once, at config load, from the YAML
/// `auth:` block (an `Authorization` header derived from
/// `type`/`token`/`token_env`) merged with the `headers:` map. The
/// parse-time shape lives in `UpstreamConfigFile`; `resolve_upstream_config` turns
/// one into the other. Verdaccio fields pnpr doesn't model yet
/// (agent options, `strict_ssl`, ...) are accepted and dropped.
#[derive(Clone)]
pub struct UpstreamConfig {
    pub url: String,
    /// Auth + custom headers, fully resolved and ready to attach to
    /// every request pnpr makes to this upstream.
    pub headers: HeaderMap,
    /// Per-upstream packument freshness window (verdaccio's `maxage`).
    /// `None` when the YAML omits it — the proxy then falls back to the
    /// global [`Config::packument_ttl`], so the existing
    /// `--packument-ttl-secs` flag still governs upstreams that don't set
    /// their own.
    pub maxage: Option<Duration>,
    /// Per-request deadline for every fetch to this upstream (verdaccio's
    /// `timeout`). Defaults to [`Self::DEFAULT_TIMEOUT`].
    pub timeout: Duration,
    /// Consecutive failures before the upstream is treated as down
    /// (verdaccio's `max_fails`). Defaults to [`Self::DEFAULT_MAX_FAILS`].
    pub max_fails: u32,
    /// How long a down upstream stays down before pnpr retries it
    /// (verdaccio's `fail_timeout`). Defaults to
    /// [`Self::DEFAULT_FAIL_TIMEOUT`].
    pub fail_timeout: Duration,
    /// Whether tarballs fetched from this upstream are written to the local
    /// mirror (verdaccio's `cache`). `false` streams them through
    /// uncached. Defaults to `true`.
    pub cache: bool,
    /// Which pnpr callers may select this upstream as a proxied private-route
    /// credential, and reach it through its `/~<name>/` registry endpoint.
    /// `None` means the upstream is registry-proxy only and is never offered as
    /// a resolver private-route credential — only upstreams that declare
    /// `access:` participate in route classification.
    pub access: Option<AccessList>,
    /// The registry's `packages:` map: the namespace it claims plus
    /// per-package `access` refinements (a `publish`/`unpublish` value is a
    /// config error — no write can land on an upstream). The registry-level
    /// gate ([`Self::access`], or `$all` for a public upstream) is the default
    /// an entry's omitted `access` falls back to.
    pub rules: PackageRules,
}

impl UpstreamConfig {
    /// Verdaccio's `timeout` default (`30s`).
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
    /// Verdaccio's `max_fails` default (`2`).
    pub const DEFAULT_MAX_FAILS: u32 = 2;
    /// Verdaccio's `fail_timeout` default (`5m`).
    pub const DEFAULT_FAIL_TIMEOUT: Duration = Duration::from_mins(5);

    /// Build a bare upstream with just a URL and headers, all tuning knobs
    /// at their verdaccio defaults. Used by the programmatic
    /// [`Config::proxy`] constructor and tests.
    pub(crate) fn with_defaults(url: String, headers: HeaderMap) -> Self {
        Self {
            url,
            headers,
            maxage: None,
            timeout: Self::DEFAULT_TIMEOUT,
            max_fails: Self::DEFAULT_MAX_FAILS,
            fail_timeout: Self::DEFAULT_FAIL_TIMEOUT,
            cache: true,
            access: None,
            rules: PackageRules::default(),
        }
    }
}

impl fmt::Debug for UpstreamConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpstreamConfig")
            .field("url", &self.url)
            .field("headers", &RedactedHeaders(&self.headers))
            .field("maxage", &self.maxage)
            .field("timeout", &self.timeout)
            .field("max_fails", &self.max_fails)
            .field("fail_timeout", &self.fail_timeout)
            .field("cache", &self.cache)
            .field("access", &self.access)
            .field("rules", &self.rules)
            .finish()
    }
}

/// Wraps a [`HeaderMap`] so its `Debug` lists header names with values
/// redacted. Upstream headers carry credentials (an `Authorization`, or
/// an API key in a custom header), and those must never reach a log
/// line, span, or diagnostic dump.
pub(crate) struct RedactedHeaders<'a>(pub(crate) &'a HeaderMap);

impl fmt::Debug for RedactedHeaders<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.0.keys().map(|name| (name.as_str(), "<redacted>"))).finish()
    }
}

/// The serving knobs of an upstream registry, in verdaccio's upstream shape for
/// the subset pnpr implements: `url`, an `auth:` block, and a free-form
/// `headers:` map. Built from an `upstream:` registry entry
/// ([`resolve_upstream_registry`]) and resolved into [`UpstreamConfig`] by
/// [`resolve_upstream_config`].
#[derive(Debug, Deserialize)]
struct UpstreamConfigFile {
    url: String,
    #[serde(default)]
    auth: Option<UpstreamAuthFile>,
    #[serde(default)]
    headers: IndexMap<String, String>,
    /// Verdaccio interval strings (`"2m"`, `"30s"`, `"1h30m"`) or a bare
    /// number of seconds; parsed by [`parse_interval`] in
    /// [`resolve_upstream_config`]. Kept as raw strings here so an unparsable
    /// value surfaces as a config error rather than a serde failure.
    #[serde(default)]
    maxage: Option<Interval>,
    #[serde(default)]
    timeout: Option<Interval>,
    #[serde(default)]
    max_fails: Option<u32>,
    #[serde(default)]
    fail_timeout: Option<Interval>,
    #[serde(default)]
    cache: Option<bool>,
    /// Which pnpr callers may select this upstream as a proxied private-route
    /// credential. Its presence is what promotes a plain proxy upstream into a
    /// resolver private-route credential exposed at `/~<name>/`.
    #[serde(default)]
    access: Option<AccessSpec>,
}

/// A verdaccio interval scalar as written in YAML: either a string
/// (`"2m"`, `"30s"`) or a bare number (a count of seconds). Both YAML
/// shapes are accepted — verdaccio configs use either — and kept as the
/// raw string so [`parse_interval`] handles them uniformly and an
/// unparsable value surfaces as a precise config error.
#[derive(Debug, Clone)]
struct Interval(String);

impl<'de> Deserialize<'de> for Interval {
    fn deserialize<De>(deserializer: De) -> Result<Self, De::Error>
    where
        De: serde::Deserializer<'de>,
    {
        struct IntervalVisitor;
        impl serde::de::Visitor<'_> for IntervalVisitor {
            type Value = Interval;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(r#"an interval string like "2m" or a number of seconds"#)
            }
            fn visit_str<DeError>(self, value: &str) -> Result<Interval, DeError> {
                Ok(Interval(value.to_string()))
            }
            fn visit_i64<DeError>(self, value: i64) -> Result<Interval, DeError> {
                Ok(Interval(value.to_string()))
            }
            fn visit_u64<DeError>(self, value: u64) -> Result<Interval, DeError> {
                Ok(Interval(value.to_string()))
            }
            fn visit_f64<DeError>(self, value: f64) -> Result<Interval, DeError> {
                Ok(Interval(value.to_string()))
            }
        }
        deserializer.deserialize_any(IntervalVisitor)
    }
}

/// The YAML `auth:` block on an upstream. `token` takes priority over
/// `token_env`; either resolves to the credential placed in the
/// `Authorization` header, encoded per [`UpstreamAuthType`].
#[derive(Debug, Deserialize)]
struct UpstreamAuthFile {
    r#type: UpstreamAuthType,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    token_env: Option<TokenEnv>,
}

/// How the resolved token is encoded into the `Authorization` header:
/// `bearer` → `Bearer <token>`, `basic` → `Basic <token>` (the token
/// is used verbatim, matching verdaccio's assumption that a `basic`
/// token is already a base64 `user:pass`).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum UpstreamAuthType {
    Bearer,
    Basic,
}

/// Verdaccio's `token_env`: either the boolean `true` (read the
/// default `NPM_TOKEN` env var) or a string naming the env var to
/// read. `false` reads nothing.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TokenEnv {
    Flag(bool),
    Named(String),
}

impl TokenEnv {
    /// Default env var name verdaccio reads for `token_env: true`.
    const DEFAULT_VAR: &'static str = "NPM_TOKEN";

    /// The env var name to read, or `None` for `token_env: false`.
    fn var_name(&self) -> Option<&str> {
        match self {
            TokenEnv::Flag(true) => Some(Self::DEFAULT_VAR),
            TokenEnv::Flag(false) => None,
            TokenEnv::Named(name) => Some(name),
        }
    }
}

/// Resolve one parsed [`UpstreamConfigFile`] into a runtime [`UpstreamConfig`],
/// baking the `auth:` credential and `headers:` map into a single
/// [`HeaderMap`]. Reads env vars (for `token_env`) through `Sys` so
/// the resolution is testable.
///
/// The auth-derived `Authorization` header is inserted first, then the
/// custom `headers:` are merged on top — so a custom `Authorization`
/// entry overrides the one derived from `auth:`, matching verdaccio's
/// merge order. A configured `auth:` block that resolves to no token,
/// an unknown header name, or a non-ASCII header value is a config
/// error rather than a silent unauthenticated request.
fn resolve_upstream_config<Sys: EnvVar>(
    name: &str,
    file: UpstreamConfigFile,
) -> Result<UpstreamConfig, RegistryError> {
    let mut headers = HeaderMap::new();
    if let Some(auth) = &file.auth {
        let token =
            resolve_upstream_token::<Sys>(auth).ok_or_else(|| RegistryError::InvalidConfig {
                reason: format!(
                    "upstream {name:?} has an auth block but no token could be resolved \
                     (set auth.token or point auth.token_env at a set env var)",
                ),
            })?;
        let value = match auth.r#type {
            UpstreamAuthType::Bearer => format!("Bearer {token}"),
            UpstreamAuthType::Basic => format!("Basic {token}"),
        };
        let value = HeaderValue::from_str(&value).map_err(|_| RegistryError::InvalidConfig {
            reason: format!("upstream {name:?} auth token is not a valid header value"),
        })?;
        headers.insert(AUTHORIZATION, value);
    }
    for (raw_name, raw_value) in &file.headers {
        let header_name = HeaderName::from_bytes(raw_name.as_bytes()).map_err(|_| {
            RegistryError::InvalidConfig {
                reason: format!("upstream {name:?} has an invalid header name {raw_name:?}"),
            }
        })?;
        let header_value =
            HeaderValue::from_str(raw_value).map_err(|_| RegistryError::InvalidConfig {
                reason: format!("upstream {name:?} header {raw_name:?} has an invalid value"),
            })?;
        headers.insert(header_name, header_value);
    }

    // Parse the verdaccio interval knobs, turning a typo'd value into a
    // config error (named for the offending field) rather than silently
    // falling back to the default.
    let parse_field = |field: &str,
                       raw: &Option<Interval>|
     -> Result<Option<Duration>, RegistryError> {
        raw.as_ref()
            .map(|Interval(value)| {
                parse_interval(value).ok_or_else(|| RegistryError::InvalidConfig {
                    reason: format!("upstream {name:?} has an invalid {field} interval {value:?}"),
                })
            })
            .transpose()
    };
    let maxage = parse_field("maxage", &file.maxage)?;
    let timeout = parse_field("timeout", &file.timeout)?.unwrap_or(UpstreamConfig::DEFAULT_TIMEOUT);
    let fail_timeout = parse_field("fail_timeout", &file.fail_timeout)?
        .unwrap_or(UpstreamConfig::DEFAULT_FAIL_TIMEOUT);

    Ok(UpstreamConfig {
        url: file.url,
        headers,
        maxage,
        timeout,
        max_fails: file.max_fails.unwrap_or(UpstreamConfig::DEFAULT_MAX_FAILS),
        fail_timeout,
        cache: file.cache.unwrap_or(true),
        access: file.access.as_ref().map(AccessSpec::to_access_list),
        // The `packages:` rules are attached by the caller
        // (`build_registries`) — this resolver only handles the serving
        // knobs shared with programmatic construction.
        rules: PackageRules::default(),
    })
}

/// Parse a verdaccio-style interval string into a [`Duration`].
///
/// Accepts the suffixes verdaccio's `parseInterval` understands —
/// `ms`, `s`, `m`, `h`, `d`, `w` — optionally chained without or with
/// whitespace (`"1h30m"`, `"2m 30s"`), and a bare number, which (like
/// verdaccio) is read as **seconds**. A trailing number with no suffix
/// is also seconds. Returns `None` for anything unparsable so the
/// caller can surface a precise config error.
fn parse_interval(raw: &str) -> Option<Duration> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // A bare number is seconds, matching verdaccio (`interval * 1000` ms).
    if let Ok(seconds) = raw.parse::<f64>() {
        // `try_from_secs_f64` rejects negative, non-finite, and
        // out-of-range values, so an absurd config (`"1e30"`) surfaces as
        // a config error rather than panicking pnpr at startup.
        return Duration::try_from_secs_f64(seconds).ok();
    }
    let mut total_seconds = 0f64;
    let bytes = raw.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        let number_start = index;
        while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') {
            index += 1;
        }
        if index == number_start {
            return None;
        }
        let number: f64 = raw[number_start..index].parse().ok()?;
        let unit_start = index;
        while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
            index += 1;
        }
        let seconds = match &raw[unit_start..index] {
            "ms" => number / 1000.0,
            "s" | "" => number,
            "m" => number * 60.0,
            "h" => number * 3600.0,
            "d" => number * 86_400.0,
            "w" => number * 604_800.0,
            _ => return None,
        };
        total_seconds += seconds;
    }
    // Fallible conversion so an overflowing compound (`"999999999999w"`)
    // is rejected as unparsable rather than panicking.
    Duration::try_from_secs_f64(total_seconds).ok()
}

/// Pick the credential for an upstream's `auth:` block: an explicit
/// `token` wins; otherwise read the env var named by `token_env`.
fn resolve_upstream_token<Sys: EnvVar>(auth: &UpstreamAuthFile) -> Option<String> {
    if let Some(token) = &auth.token {
        return non_empty_token(token);
    }
    let var_name = auth.token_env.as_ref()?.var_name()?;
    Sys::var(var_name).and_then(|token| non_empty_token(&token))
}

fn non_empty_token(token: &str) -> Option<String> {
    (!token.trim().is_empty()).then(|| token.to_string())
}

/// One `packages:` map value: `access` / `publish` / `unpublish` are verdaccio
/// permission lists (built-in groups like `$all` / `$authenticated` /
/// `$anonymous`, plus usernames / group names), compiled into the owning
/// registry's [`PackageRules`]. An omitted field falls back to the
/// registry-level default. The map key set doubles as the registry's declared
/// namespace, so one declaration routes, filters, and authorizes.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageAccess {
    pub access: Option<AccessSpec>,
    pub publish: Option<AccessSpec>,
    pub unpublish: Option<AccessSpec>,
}

/// A YAML string-or-list value. Verdaccio accepts either a single
/// space-separated string (`access: $authenticated admin`) or a sequence
/// (`access: [$authenticated, admin]`); both normalize to the same ordered
/// token list.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum AccessSpec {
    One(String),
    Many(Vec<String>),
}

impl AccessSpec {
    fn to_access_list(&self) -> AccessList {
        match self {
            AccessSpec::One(spec) => AccessList::parse(spec),
            // Each element may itself be space-separated; flatten so
            // `[a b, c]` and `[a, b, c]` agree.
            AccessSpec::Many(items) => {
                AccessList::from_tokens(items.iter().flat_map(|item| item.split_whitespace()))
            }
        }
    }

    /// The tokens in declared order, each element flattened on whitespace
    /// (so `[a b, c]` and `[a, b, c]` agree). Unlike [`Self::to_access_list`]
    /// — which builds an unordered permission *set* — this preserves order,
    /// for consumers (the `groups:` membership lists) where the declared
    /// sequence is meaningful.
    fn to_ordered_tokens(&self) -> Vec<&str> {
        match self {
            AccessSpec::One(spec) => spec.split_whitespace().collect(),
            AccessSpec::Many(items) => {
                items.iter().flat_map(|item| item.split_whitespace()).collect()
            }
        }
    }
}

/// Disk shape of the `routes:` block.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoutesFile {
    #[serde(default)]
    public: Vec<PublicRouteFile>,
}

#[derive(Debug, Deserialize)]
struct PublicRouteFile {
    #[serde(default)]
    registry: Option<String>,
    #[serde(default)]
    package: Option<String>,
}

/// Disk shape of one `registries:` entry, discriminated by an internal `type:` tag
/// (`hosted` / `upstream` / `router`). The tag selects exactly one kind, so
/// "declared none or more than one" is unrepresentable — no runtime count check.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum RegistryFile {
    Hosted(HostedFile),
    // Boxed: `UpstreamFile` is far larger than the other kinds, so an unboxed
    // variant would bloat every `RegistryFile`.
    Upstream(Box<UpstreamFile>),
    Router(RouterFile),
}

/// Disk shape of a `hosted:` registry.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostedFile {
    /// Storage namespace for this registry's packages, so two hosted registries can
    /// hold the same `name@version` without colliding. Omitted ⇒ the flat
    /// `storage` root (`""`).
    #[serde(default)]
    org: String,
    /// The registry-level default: who may read this registry's packages when
    /// no `packages:` entry refines it. Omitted ⇒ `$all`.
    #[serde(default)]
    access: Option<AccessSpec>,
    /// The names this registry serves and accepts publishes for — its
    /// namespace — with optional per-package `access`/`publish`/`unpublish`
    /// rules as values (`{}` or null ⇒ the registry defaults). The most
    /// specific matching key wins; key order carries no meaning. Omitted ⇒
    /// every name, default rules.
    #[serde(default)]
    packages: IndexMap<String, Option<PackageAccess>>,
}

/// Disk shape of an `upstream:` registry — one external origin. Mirrors an
/// upstream's tuning knobs plus `public` (an anonymous, no-credential origin)
/// and `access` (which pnpr callers may reach a private one).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpstreamFile {
    url: String,
    /// An anonymous, world-readable origin (e.g. the public npm registry).
    /// Mutually exclusive with `auth`.
    #[serde(default)]
    public: bool,
    #[serde(default)]
    auth: Option<UpstreamAuthFile>,
    #[serde(default)]
    headers: IndexMap<String, String>,
    #[serde(default)]
    maxage: Option<Interval>,
    #[serde(default)]
    timeout: Option<Interval>,
    #[serde(default)]
    max_fails: Option<u32>,
    #[serde(default)]
    fail_timeout: Option<Interval>,
    #[serde(default)]
    cache: Option<bool>,
    /// Which pnpr callers may reach this registry at `/~<name>/`. Required for a
    /// non-`public` upstream (otherwise no one could be authorized to use it).
    #[serde(default)]
    access: Option<AccessSpec>,
    /// The names that may be requested through this registry — its namespace —
    /// with optional per-package `access` refinements as values (`{}` or null
    /// ⇒ the registry default; a `publish`/`unpublish` value is a config
    /// error, since no write can land on an upstream). Omitted ⇒ every name.
    /// Bounding a private upstream stops an authorized caller from pulling
    /// arbitrary public names through its server-owned credential.
    #[serde(default)]
    packages: IndexMap<String, Option<PackageAccess>>,
}

/// Disk shape of a `router:` registry: an ordered list of concrete registry names.
/// The first source whose declared patterns claim a package serves it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouterFile {
    #[serde(default)]
    sources: Vec<String>,
}

/// Disk shape of the YAML file. Fields verdaccio supports but
/// pnpr doesn't (`auth`, `web`, `plugins`, `middlewares`,
/// `logs`, `secret`) are accepted and silently dropped via
/// `#[serde(default)]` on the fields we care about plus
/// `#[serde(deny_unknown_fields)]` *not* being set — so the same
/// `config.yaml` works for both servers.
#[derive(Debug, Deserialize)]
struct ConfigFile {
    #[serde(default = "default_storage_string")]
    storage: String,
    /// Disposable proxy-cache root. Not a verdaccio key — when omitted
    /// it defaults to a `.pnpr-cache` subdirectory of `storage`.
    #[serde(default)]
    cache: Option<String>,
    /// pnpr-only block: store the hosted (published) packages in an
    /// S3-compatible object store instead of `storage`. Absent on a
    /// stock verdaccio config (silently ignored there).
    #[serde(default)]
    s3: Option<S3Settings>,
    /// pnpr-only block: back the auth record stores (users + tokens)
    /// with a shared SQL database. Absent on a stock verdaccio config
    /// (silently ignored there).
    #[serde(default)]
    backend: Option<BackendFile>,
    /// pnpr-only local OSV database settings.
    #[serde(default)]
    osv: OsvFile,
    /// pnpr-only feature toggle for the resolver surface. On unless
    /// explicitly disabled; absent on a stock verdaccio config, so it
    /// stays enabled there. `Option` so a bare `resolver:` (which YAML
    /// parses as null) is accepted as "default" rather than failing to
    /// deserialize into the struct.
    #[serde(default)]
    resolver: Option<FeatureFile>,
    /// pnpr registries: hosted, upstream, and router origins, each
    /// exposed at `/~<name>/`. The only routing surface — there is no legacy
    /// `upstreams:`/`packages: proxy:` fallback.
    #[serde(default)]
    registries: IndexMap<String, RegistryFile>,
    /// The registry the path-less base URL aliases. Absent ⇒ the bare host has no
    /// registry and clients must address a `/~<name>/`.
    #[serde(default, rename = "defaultRegistry")]
    default_registry: Option<String>,
    /// The removed top-level ACL block, kept only to *reject* it loudly.
    /// Per-package rules live on each registry's `packages:` map now; a
    /// config still carrying the global block previously enforced access
    /// with it, so silently dropping the key (the fate of unknown verdaccio
    /// fields) would be a security regression — private packages would
    /// quietly open up on upgrade.
    #[serde(default)]
    packages: Option<serde::de::IgnoredAny>,
    /// pnpr-only static groups: each key is a group/team name and each
    /// value is the list of pnpr usernames in that group.
    #[serde(default)]
    groups: IndexMap<String, AccessSpec>,
    /// pnpr-only: which fetch routes the resolution cache treats as
    /// public. Absent on a stock verdaccio config (built-in defaults
    /// apply).
    #[serde(default)]
    routes: Option<RoutesFile>,
    /// Verdaccio's `secret:` — reused here to key the private
    /// resolution-cache HMAC. A random per-process secret is used when
    /// absent.
    #[serde(default)]
    secret: Option<String>,
    #[serde(default)]
    auth: AuthFile,
    /// Verdaccio 6+ shape: `log:` is a single object at the top
    /// level, not a list. The older `logs:` list shape is
    /// intentionally not accepted.
    #[serde(default)]
    log: Option<LogEntryFile>,
}

/// The YAML `log:` object. Mirrors verdaccio 6's logger config.
#[derive(Debug, Deserialize)]
struct LogEntryFile {
    #[serde(default = "default_log_type")]
    r#type: String,
    #[serde(default)]
    format: Option<LogFormat>,
    #[serde(default)]
    level: Option<LogLevel>,
}

fn default_log_type() -> String {
    LogConfig::STDOUT_SINK.to_string()
}

#[derive(Debug, Default, Deserialize)]
struct AuthFile {
    #[serde(default)]
    htpasswd: HtpasswdFile,
    #[serde(default)]
    tokens: TokensFile,
}

#[derive(Debug, Default, Deserialize)]
struct BackendFile {
    #[serde(default)]
    libsql: Option<LibsqlSettings>,
    #[serde(default)]
    postgres: Option<SqlBackendFile>,
    #[serde(default)]
    postgresql: Option<SqlBackendFile>,
    #[serde(default)]
    mysql: Option<SqlBackendFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SqlBackendFile {
    url: String,
    #[serde(default)]
    max_connections: Option<u32>,
    #[serde(default)]
    timeout: Option<Interval>,
    #[serde(default)]
    startup_timeout: Option<Interval>,
}

#[derive(Debug, Default, Deserialize)]
struct HtpasswdFile {
    #[serde(default)]
    file: Option<String>,
    /// `i64` so the verdaccio sentinel `-1` (registration disabled)
    /// parses; anything `≥ 0` becomes a hard cap.
    #[serde(default)]
    max_users: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct TokensFile {
    #[serde(default)]
    file: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OsvFile {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    path: Option<String>,
}

/// Disk shape of the `resolver:` feature block. A bare `enabled` today;
/// sub-feature keys can be added later. The field and the whole-block
/// defaults are both `enabled: true`, so omitting the block — or writing
/// `resolver:` with no body — keeps the surface on.
/// `deny_unknown_fields` so a typo like `resolver: { enable: false }`
/// (note: `enable`, not `enabled`) is a loud config error rather than
/// silently leaving the surface enabled — the toggle scopes which
/// endpoints are exposed, so a silent default-on is a security footgun.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FeatureFile {
    #[serde(default = "default_true")]
    enabled: bool,
}

impl Default for FeatureFile {
    fn default() -> Self {
        Self { enabled: true }
    }
}

fn default_true() -> bool {
    true
}

/// The namespace [`Config::proxy`] declares on its flat-root hosted org, so
/// those names resolve locally rather than to the npm upstream: the
/// registry-mock fixture scopes plus the one unscoped fixture. Kept in sync
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
    let authenticated = || Some(AccessList::parse("$authenticated"));
    let mut rules: Vec<PackageRule> = REGISTRY_MOCK_LOCAL_PATTERNS
        .iter()
        .map(|pattern| PackageRule {
            pattern: PackagePattern::parse(pattern)
                .expect("valid built-in fixture registry pattern"),
            access: (*pattern == "@private/*").then(authenticated).flatten(),
            publish: (*pattern == "@private/*").then(authenticated).flatten(),
            unpublish: (*pattern == "@private/*").then(authenticated).flatten(),
        })
        .collect();
    rules.push(PackageRule {
        pattern: PackagePattern::parse("@pnpm.e2e/needs-auth")
            .expect("valid built-in fixture registry pattern"),
        access: authenticated(),
        publish: authenticated(),
        unpublish: authenticated(),
    });
    PackageRules::new(rules, None).with_default_unpublish(AccessList::parse("$authenticated"))
}

impl Config {
    /// Default `listen` when one isn't supplied by the caller.
    pub const DEFAULT_LISTEN: &'static str = "127.0.0.1:7677";
    /// Default packument TTL — five minutes, matching the historical
    /// proxy-mode default.
    pub const DEFAULT_PACKUMENT_TTL: Duration = Duration::from_mins(5);

    /// Build a proxy-mode config in the registry-mock shape: the fixture scopes
    /// (and the one unscoped fixture) are the declared namespace of a flat-root
    /// hosted org over `storage`, while every other name proxies to the
    /// pattern-less `npmjs` upstream. The path-less base aliases the `main`
    /// router. Kept for callers that don't use YAML config (notably pacquet's
    /// test registry, whose fixtures are served locally while real npm packages
    /// fall through to npmjs). The local pattern set mirrors the fixture
    /// subset of the bundled `config.yaml` `local` registry
    /// (`REGISTRY_MOCK_LOCAL_PATTERNS`); the YAML additionally claims the
    /// exact names the TS test suite publishes, which never reach this
    /// constructor.
    #[must_use]
    pub fn proxy(listen: SocketAddr, storage: PathBuf) -> Self {
        let mut upstreams = IndexMap::new();
        upstreams.insert(
            "npmjs".to_string(),
            UpstreamConfig::with_defaults(
                "https://registry.npmjs.org".to_string(),
                HeaderMap::new(),
            ),
        );
        let rules = registry_mock_rules();
        let local_patterns = rules.patterns();
        let mut hosted = IndexMap::new();
        hosted.insert("local".to_string(), HostedConfig { org: String::new(), rules });
        let graph = [
            ("local".to_string(), Registry::Hosted { patterns: local_patterns }),
            ("npmjs".to_string(), Registry::Upstream { patterns: Vec::new() }),
            (
                "main".to_string(),
                Registry::Router { sources: vec!["local".to_string(), "npmjs".to_string()] },
            ),
        ];
        let registries = Registries::new(graph.into_iter().collect(), Some("main".to_string()));
        Self {
            listen,
            public_url: format!("http://{listen}"),
            cache_storage: default_cache_dir(&storage),
            storage,
            upstreams,
            groups: AccessGroups::default(),
            packument_ttl: Self::DEFAULT_PACKUMENT_TTL,
            auth: AuthConfig::default(),
            logs: LogConfig::default(),
            hosted_store: HostedStoreConfig::Fs,
            backend: BackendConfig::Local,
            osv: OsvConfig::default(),
            registry: RegistryFeature::default(),
            resolver: ResolverFeature::default(),
            route_policy: RoutePolicy::default(),
            resolution_cache_secret: random_secret(),
            registries,
            hosted,
        }
    }

    /// Build a static-mode config that serves `storage` verbatim: one
    /// pattern-less hosted registry over the storage root (an empty `org`
    /// namespace == the flat root), the sole source of a router that the
    /// path-less base aliases. Every package resolves to that one hosted
    /// origin — no upstream, no fall-through.
    #[must_use]
    pub fn static_serve(listen: SocketAddr, storage: PathBuf) -> Self {
        let mut hosted = IndexMap::new();
        // The graph entry below is pattern-less — static mode claims and
        // serves every name in `storage` — while the rules still carry the
        // registry-mock protections (`@private/*`, `@pnpm.e2e/needs-auth`,
        // authenticated unpublish). Programmatic configs may split the
        // namespace (graph) from the rules like this; YAML derives both from
        // one `packages:` map.
        hosted.insert(
            "local".to_string(),
            HostedConfig { org: String::new(), rules: registry_mock_rules() },
        );
        let graph = [
            ("local".to_string(), Registry::Hosted { patterns: Vec::new() }),
            ("main".to_string(), Registry::Router { sources: vec!["local".to_string()] }),
        ];
        let registries = Registries::new(graph.into_iter().collect(), Some("main".to_string()));
        Self {
            listen,
            public_url: format!("http://{listen}"),
            cache_storage: default_cache_dir(&storage),
            storage,
            upstreams: IndexMap::new(),
            groups: AccessGroups::default(),
            packument_ttl: Self::DEFAULT_PACKUMENT_TTL,
            auth: AuthConfig::default(),
            logs: LogConfig::default(),
            hosted_store: HostedStoreConfig::Fs,
            backend: BackendConfig::Local,
            osv: OsvConfig::default(),
            registry: RegistryFeature::default(),
            resolver: ResolverFeature::default(),
            route_policy: RoutePolicy::default(),
            resolution_cache_secret: random_secret(),
            registries,
            hosted,
        }
    }

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

    fn from_yaml_with_overrides(
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

    fn from_default_yaml_with_overrides(
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
    /// the shared [`pacquet_config_dir::config_dir`]) under a `pnpr`
    /// leaf, so an operator who knows where `pnpm config` looks knows
    /// where pnpr looks too.
    pub fn auto_config_path() -> Option<PathBuf> {
        let dir = pacquet_config_dir::config_dir(
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
    fn from_yaml_str(
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

    fn from_yaml_str_with_overrides(
        raw: &str,
        base_dir: &Path,
        listen: SocketAddr,
        public_url: Option<String>,
        overrides: FeatureOverrides,
    ) -> Result<Self, RegistryError> {
        let (substituted, unresolved) = env_replace_lossy::<SystemEnv>(raw);
        if !unresolved.is_empty() {
            tracing::warn!(?unresolved, "config references unset environment variables");
        }
        let file: ConfigFile = serde_saphyr::from_str(&substituted)
            .map_err(|err| RegistryError::InvalidConfig { reason: err.to_string() })?;
        let storage = resolve_relative(&file.storage, base_dir);
        let cache_storage = file
            .cache
            .as_deref()
            .map_or_else(|| default_cache_dir(&storage), |raw| resolve_relative(raw, base_dir));
        let hosted_store = match &file.s3 {
            Some(s3) => {
                HostedStoreConfig::S3 { store: build_s3_store(s3)?, prefix: s3.normalized_prefix() }
            }
            None => HostedStoreConfig::Fs,
        };
        let backend = build_backend_config(file.backend, base_dir)?;
        let public_url = public_url.unwrap_or_else(|| format!("http://{listen}"));
        let auth = build_auth_config(&file.auth, base_dir);
        let logs = build_log_config(file.log.as_ref());
        let groups = build_groups(&file.groups);
        // The global ACL block is gone, not ignorable: it used to *enforce*
        // access, so dropping it like an unknown verdaccio key would silently
        // open previously-gated packages on upgrade. Fail loudly instead,
        // naming the replacement.
        if file.packages.is_some() {
            return Err(RegistryError::InvalidConfig {
                reason: "the top-level `packages:` block was removed: declare per-package rules \
                         on the registry that serves them, as `registries.<name>.packages` \
                         (pattern keys, `access`/`publish`/`unpublish` values)"
                    .to_string(),
            });
        }
        let osv = build_osv_config(&file.osv, base_dir);
        // The npm-registry surface is derived, not configured: served iff
        // at least one registry is declared (no registries ⇒ nothing to serve),
        // minus the per-tier `--disable-registry` override. Folding the
        // override in here lets the registry-only work below (upstream
        // credential resolution) key off effective enablement.
        let registry =
            RegistryFeature { enabled: !file.registries.is_empty() && !overrides.disable_registry };
        let resolver = ResolverFeature {
            enabled: file.resolver.unwrap_or_default().enabled && !overrides.disable_resolver,
        };
        // Upstream registries (and the credentials some carry) are resolved by
        // `build_registries` below into this map. Resolving an upstream registry's
        // `auth` is strict — an unresolvable token is a config error — so a
        // resolver-only server (which serves no registry routes) skips the
        // credential resolution rather than carry upstream secrets it never
        // uses. The registry *graph* is still built and validated either way, so
        // a misconfigured router or org fails startup on every tier, not only
        // when the registry surface happens to be enabled.
        let mut upstreams: IndexMap<String, UpstreamConfig> = IndexMap::new();
        let (hosted, registries) = build_registries(
            &mut upstreams,
            file.registries,
            file.default_registry,
            registry.enabled,
        )?;
        let route_policy = build_route_policy(file.routes);
        let resolution_cache_secret = resolution_secret(file.secret.as_deref())?;
        let config = Self {
            listen,
            public_url,
            storage,
            cache_storage,
            upstreams,
            groups,
            packument_ttl: Self::DEFAULT_PACKUMENT_TTL,
            auth,
            logs,
            hosted_store,
            backend,
            osv,
            registry,
            resolver,
            route_policy,
            resolution_cache_secret,
            registries,
            hosted,
        };
        config.ensure_a_feature_is_enabled()?;
        Ok(config)
    }

    /// At least one top-level surface must be served; a server with no
    /// registry surface (no registries declared, or `--disable-registry`) and
    /// the resolver disabled would answer only `/-/ping` and the account
    /// endpoints. Checked at config load and again in the serve/router
    /// entry points for programmatically built configs.
    pub fn ensure_a_feature_is_enabled(&self) -> Result<(), RegistryError> {
        if self.registry.enabled || self.resolver.enabled {
            Ok(())
        } else {
            Err(RegistryError::InvalidConfig {
                reason: "nothing to serve: the npm-registry surface is off (no `registries:` \
                         declared, or `--disable-registry`) and the resolver is disabled"
                    .to_string(),
            })
        }
    }

    /// Ready the registry graph for serving: fold every upstream into the graph
    /// as a pattern-less upstream registry, then apply every invariant YAML
    /// loading enforces — URL-safe registry names, path-safe and collision-free
    /// hosted `org` namespaces, and the graph validation itself. This covers
    /// embedders that build [`Self::upstreams`], [`Self::hosted`], or
    /// [`Self::registries`] programmatically, so [`Registries::resolve`] is the
    /// only dispatch table for `/~<name>/` traffic and a programmatically-built
    /// config fails closed like a YAML load. An embedder that wants a
    /// namespace bound on an upstream declares its registry entry (with
    /// patterns) before serving.
    pub fn ensure_valid_registry_graph(&mut self) -> Result<(), RegistryError> {
        for name in self.upstreams.keys() {
            self.registries.ensure_upstream(name);
            // The fold never overwrites, so a graph entry already declared
            // under this name must actually be the upstream — otherwise two
            // different origins would share one `/~<name>/` identity, with
            // the dormant upstream's credential still offered by the
            // resolver. YAML loading rejects the same collision while
            // building the graph.
            if !matches!(self.registries.get(name), Some(Registry::Upstream { .. })) {
                return Err(RegistryError::InvalidConfig {
                    reason: format!(
                        "upstream registry {name:?} collides with a non-upstream registry of \
                         the same name",
                    ),
                });
            }
        }
        for name in self.registries.names() {
            validate_registry_name(name)?;
            // A concrete registry needs its serving config — a hosted graph
            // entry without its `hosted` table row (or an upstream without
            // its serving entry) would answer every request not-found at
            // runtime. YAML loading builds both sides together; catch a
            // programmatically-built mismatch at startup. Upstream backing
            // is only required when the registry surface is enabled: a
            // resolver-only tier deliberately skips upstream (credential)
            // resolution and never serves `/~<name>/` content.
            match self.registries.get(name) {
                Some(Registry::Hosted { .. }) if !self.hosted.contains_key(name) => {
                    return Err(RegistryError::InvalidConfig {
                        reason: format!(
                            "hosted registry {name:?} has no entry in the hosted serving table; \
                             every request to it would be not-found",
                        ),
                    });
                }
                Some(Registry::Upstream { .. })
                    if self.registry.enabled && !self.upstreams.contains_key(name) =>
                {
                    return Err(RegistryError::InvalidConfig {
                        reason: format!(
                            "upstream registry {name:?} has no serving config (URL, credentials); \
                             every request to it would fail",
                        ),
                    });
                }
                _ => {}
            }
        }
        for (index, (name, hosted)) in self.hosted.iter().enumerate() {
            validate_registry_name(name)?;
            validate_org_namespace(name, &hosted.org)?;
            // The mirror of the upstream collision above: a hosted serving row
            // under a name the graph declares as a different kind would leave
            // `/~<name>/` serving one origin while the row describes another.
            // (A row with no graph entry at all is merely dormant.)
            if let Some(kind) = self.registries.get(name)
                && !matches!(kind, Registry::Hosted { .. })
            {
                return Err(RegistryError::InvalidConfig {
                    reason: format!(
                        "hosted registry {name:?} collides with a non-hosted registry of the \
                         same name",
                    ),
                });
            }
            if let Some((other, _)) =
                self.hosted.iter().take(index).find(|(_, existing)| existing.org == hosted.org)
            {
                return Err(org_collision_error(name, &hosted.org, other));
            }
        }
        for (name, upstream) in &self.upstreams {
            // Mirror the YAML rule (`resolve_upstream_registry`): an upstream
            // with no `access:` gate is publicly reachable at `/~<name>/`, and
            // a public origin sends no request headers — any header can carry
            // a credential, and an ungated endpoint would let every caller
            // spend it (a confused deputy).
            if upstream.access.is_none() && !upstream.headers.is_empty() {
                return Err(RegistryError::InvalidConfig {
                    reason: format!(
                        "upstream registry {name:?} sends custom headers but declares no \
                         `access:` gate; a publicly reachable upstream must send none",
                    ),
                });
            }
        }
        self.registries.validate().map_err(|err| registry_err(&err))
    }

    /// Build the caller identity used by access policies after a bearer
    /// token has authenticated `username`.
    #[must_use]
    pub fn identity_for_user(&self, username: impl Into<String>) -> Identity {
        self.groups.identity_for(username.into())
    }
}

/// Build the runtime [`AuthConfig`] from the YAML `auth:` block.
/// Relative paths are resolved against `base_dir` so a path like
/// `./htpasswd` lives next to the config file (verdaccio's
/// convention). When `auth.htpasswd.file` is set but
/// `auth.tokens.file` is not, tokens default to a `tokens.db`
/// sibling of the htpasswd file — keeping credentials co-located in
/// one directory the operator can lock down (`chmod 600`).
fn build_auth_config(file: &AuthFile, base_dir: &Path) -> AuthConfig {
    let htpasswd_file = file.htpasswd.file.as_deref().map(|raw| resolve_relative(raw, base_dir));
    let tokens_file = file
        .tokens
        .file
        .as_deref()
        .map(|raw| resolve_relative(raw, base_dir))
        .or_else(|| htpasswd_file.as_deref().map(default_tokens_path_sibling_of));
    AuthConfig {
        htpasswd: HtpasswdConfig {
            file: htpasswd_file,
            max_users: file.htpasswd.max_users.map_or(MaxUsers::Disabled, MaxUsers::from_yaml),
        },
        tokens: TokensConfig { file: tokens_file },
    }
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

/// Turn a static [`RegistryConfigError`] into the server-wide config error so a
/// bad registry set fails startup and config reload like any other.
fn registry_err(err: &RegistryConfigError) -> RegistryError {
    RegistryError::InvalidConfig { reason: err.to_string() }
}

/// Build the validated [`Registries`] graph (and the hosted table) from the
/// resolved upstreams and the `registries:` block. Every upstream is an upstream
/// registry; `registries:` adds hosted, further upstream, and router registries.
/// Upstream registries declared under `registries:` are folded into `upstreams` so they
/// reuse the same serving and route-classification machinery. Fails closed on
/// any name collision, malformed registry, or invalid routing graph.
///
/// With `resolve_upstreams` false (the registry surface is disabled), an
/// upstream registry still joins the graph for validation but its credentials
/// and serving config are not resolved — a resolver-only tier must not fail
/// on (or carry) upstream secrets it never uses.
fn build_registries(
    upstreams: &mut IndexMap<String, UpstreamConfig>,
    registry_files: IndexMap<String, RegistryFile>,
    default_registry: Option<String>,
    resolve_upstreams: bool,
) -> Result<(IndexMap<String, HostedConfig>, Registries), RegistryError> {
    let mut hosted: IndexMap<String, HostedConfig> = IndexMap::new();
    let mut graph: IndexMap<String, Registry> = IndexMap::new();
    // Every configured upstream is, by definition, an upstream registry addressable
    // at `/~<name>/`. No declared patterns ⇒ it serves every name.
    for name in upstreams.keys() {
        validate_registry_name(name)?;
        graph.insert(name.clone(), Registry::Upstream { patterns: Vec::new() });
    }
    for (name, file) in registry_files {
        validate_registry_name(&name)?;
        if graph.contains_key(&name) {
            return Err(RegistryError::InvalidConfig {
                reason: format!(
                    "registry {name:?} collides with another registry or upstream of the same name",
                ),
            });
        }
        match file {
            RegistryFile::Hosted(registry) => {
                validate_org_namespace(&name, &registry.org)?;
                if let Some((other, _)) = hosted
                    .iter()
                    .find(|(_, existing): &(_, &HostedConfig)| existing.org == registry.org)
                {
                    return Err(org_collision_error(&name, &registry.org, other));
                }
                let access = registry.access.as_ref().map(AccessSpec::to_access_list);
                let rules = build_rules(&name, &registry.packages, access)?;
                let patterns = rules.patterns();
                hosted.insert(name.clone(), HostedConfig { org: registry.org, rules });
                graph.insert(name, Registry::Hosted { patterns });
            }
            RegistryFile::Upstream(upstream) => {
                // The registry-level default the rules fall back to: the
                // upstream's `access:` gate, or `$all` for a public origin.
                // Built before the `resolve_upstreams` fork so the graph
                // carries the namespace on every tier, and so a
                // `publish`/`unpublish` value — a write rule on a registry no
                // write can land on — fails startup on every tier too.
                let access = upstream.access.as_ref().map(AccessSpec::to_access_list);
                let rules = build_rules(&name, &upstream.packages, access)?;
                if rules.refines_writes() {
                    return Err(RegistryError::InvalidConfig {
                        reason: format!(
                            "upstream registry {name:?} declares `publish`/`unpublish` rules in \
                             its `packages:` map; writes can never land on an upstream",
                        ),
                    });
                }
                let patterns = rules.patterns();
                if resolve_upstreams {
                    let mut resolved = resolve_upstream_registry::<SystemEnv>(&name, *upstream)?;
                    resolved.rules = rules;
                    upstreams.insert(name.clone(), resolved);
                }
                graph.insert(name, Registry::Upstream { patterns });
            }
            RegistryFile::Router(router) => {
                graph.insert(name, Registry::Router { sources: router.sources });
            }
        }
    }
    let registries = Registries::new(graph, default_registry);
    registries.validate().map_err(|err| registry_err(&err))?;
    Ok((hosted, registries))
}

/// Two hosted registries sharing an `org` would read and write the same
/// storage namespace, so a package published to one would surface through the
/// other — breaking the declared-provenance isolation. Rejected at load,
/// whether the config came from YAML or an embedder.
fn org_collision_error(name: &str, org: &str, other: &str) -> RegistryError {
    RegistryError::InvalidConfig {
        reason: format!(
            "hosted registry {name:?} reuses the `org` namespace {org:?} already claimed by \
             registry {other:?}; two hosted registries cannot share a namespace",
        ),
    }
}

/// A registry's name is addressed as the single URL path segment `/~<name>/` and
/// is embedded verbatim into rewritten `dist.tarball` URLs, so it must be one
/// URL-safe segment. A name that can't survive that round trip is rejected at
/// load rather than becoming an unreachable registry (`/` splits it across
/// segments), a URL-parsing ambiguity (`?`, `#`, `%`, whitespace, control
/// characters), or a path-meaningful component intermediaries may normalize
/// away (`.`, `..`, a Windows drive prefix).
fn validate_registry_name(name: &str) -> Result<(), RegistryError> {
    let safe = crate::package_name::is_safe_path_segment(name)
        && !name.contains(['%', '?', '#'])
        && !name.contains(|ch: char| ch.is_whitespace() || ch.is_control());
    if safe {
        return Ok(());
    }
    Err(RegistryError::InvalidConfig {
        reason: format!(
            "registry name {name:?} is not a single URL-safe path segment: it is served at \
             `/~<name>/`, so it cannot be empty, `.` or `..`, start with `.`, or contain `/`, \
             `\\`, `:`, `%`, `?`, `#`, whitespace, or control characters",
        ),
    })
}

/// A hosted registry's `org` becomes a storage path/key segment (`Storage::for_hosted`),
/// so it must be empty (the flat root) or one safe component under the same
/// rules as every other on-disk segment (no separators, traversal, leading
/// dot, or Windows drive prefix) — otherwise a crafted config could read or
/// write outside the storage root. The leading-dot rule also keeps an org
/// from aliasing the reserved dot-directories inside the storage root (the
/// default `.pnpr-cache` wipeable cache and the `.pnpr-journal` commit
/// journal), which would put authoritative packages under a path an operator
/// is told is safe to delete.
fn validate_org_namespace(name: &str, org: &str) -> Result<(), RegistryError> {
    if org.is_empty() || crate::package_name::is_safe_path_segment(org) {
        return Ok(());
    }
    Err(RegistryError::InvalidConfig {
        reason: format!(
            "hosted registry {name:?} has an invalid `org` {org:?}: it must be a single path-safe \
             segment (no `/`, `\\`, `:`, leading `.`, or traversal)",
        ),
    })
}

/// Compile a concrete registry's `packages:` map into its [`PackageRules`]:
/// keys parsed into the decidable [`PackagePattern`] language, values into
/// per-package permission rules (`{}`/null ⇒ all fields default). Selection
/// is by specificity, so key order carries no meaning; a duplicate key is the
/// only within-registry error, and the YAML parser already rejects literal
/// duplicates in one mapping — the check here guards the graph invariant for
/// any other construction path. Routing-graph checks are handled later by
/// [`Registries::validate`] once the whole graph exists.
fn build_rules(
    registry: &str,
    packages: &IndexMap<String, Option<PackageAccess>>,
    default_access: Option<AccessList>,
) -> Result<PackageRules, RegistryError> {
    let rules = packages
        .iter()
        .map(|(pattern, rule)| {
            let pattern = PackagePattern::parse(pattern).map_err(|err| {
                RegistryError::InvalidConfig { reason: format!("registry {registry:?}: {err}") }
            })?;
            let fields = rule.as_ref();
            Ok(PackageRule {
                pattern,
                access: fields
                    .and_then(|fields| fields.access.as_ref())
                    .map(AccessSpec::to_access_list),
                publish: fields
                    .and_then(|fields| fields.publish.as_ref())
                    .map(AccessSpec::to_access_list),
                unpublish: fields
                    .and_then(|fields| fields.unpublish.as_ref())
                    .map(AccessSpec::to_access_list),
            })
        })
        .collect::<Result<Vec<_>, RegistryError>>()?;
    Ok(PackageRules::new(rules, default_access))
}

/// Resolve an `upstream:` registry into the shared [`UpstreamConfig`] runtime shape.
/// A `public` upstream is anonymous and world-readable (no credential, no
/// access gate); a non-`public` one must declare `access:` naming who may
/// reach it at `/~<name>/`. Declaring both `public` and `auth` is rejected —
/// a public origin sends no credential.
fn resolve_upstream_registry<Sys: EnvVar>(
    name: &str,
    file: UpstreamFile,
) -> Result<UpstreamConfig, RegistryError> {
    // A public origin is anonymous and shared, so every credential-bearing or
    // access-gating knob contradicts `public: true` and must fail closed rather
    // than be silently ignored (which would send a credential to, or expose, a
    // supposedly-public origin).
    if file.public && file.auth.is_some() {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "upstream registry {name:?} is `public` but also declares `auth`; a public origin \
                 sends no credential",
            ),
        });
    }
    if file.public && file.access.is_some() {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "upstream registry {name:?} is `public` but also declares `access`; a public origin \
                 is reachable anonymously",
            ),
        });
    }
    if file.public && !file.headers.is_empty() {
        // A public origin is fetched anonymously, so it sends no request headers
        // at all. Rejecting *any* custom header (not just `Authorization`) closes
        // the door on a credential smuggled through `X-Api-Key`, a cookie, or any
        // other header on a registry that is meant to be reachable anonymously.
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "upstream registry {name:?} is `public` but declares custom `headers`; a public \
                 origin is fetched anonymously and sends none",
            ),
        });
    }
    if !file.public && file.access.is_none() {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "upstream registry {name:?} must set `public: true` or declare `access:` (who may \
                 reach it at /~{name}/)",
            ),
        });
    }
    let access = if file.public { None } else { file.access };
    let upstream_config_file = UpstreamConfigFile {
        url: file.url,
        auth: file.auth,
        headers: file.headers,
        maxage: file.maxage,
        timeout: file.timeout,
        max_fails: file.max_fails,
        fail_timeout: file.fail_timeout,
        cache: file.cache,
        access,
    };
    resolve_upstream_config::<Sys>(name, upstream_config_file)
}

fn build_groups(file: &IndexMap<String, AccessSpec>) -> AccessGroups {
    let mut groups = AccessGroups::default();
    for (group, members) in file {
        for username in members.to_ordered_tokens() {
            groups.add_user_to_group(username, group);
        }
    }
    groups
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

fn build_backend_config(
    file: Option<BackendFile>,
    base_dir: &Path,
) -> Result<BackendConfig, RegistryError> {
    let Some(file) = file else {
        return Ok(BackendConfig::Local);
    };
    let mut selected = Vec::new();
    if let Some(mut settings) = file.libsql {
        resolve_libsql_paths(&mut settings, base_dir);
        selected.push(("libsql", BackendConfig::Libsql(settings)));
    }
    if let Some(settings) = file.postgres {
        selected.push((
            "postgres",
            BackendConfig::Postgres(build_sql_backend_settings("postgres", settings)?),
        ));
    }
    if let Some(settings) = file.postgresql {
        selected.push((
            "postgresql",
            BackendConfig::Postgres(build_sql_backend_settings("postgresql", settings)?),
        ));
    }
    if let Some(settings) = file.mysql {
        selected
            .push(("mysql", BackendConfig::Mysql(build_sql_backend_settings("mysql", settings)?)));
    }
    match selected.len() {
        0 => Err(RegistryError::InvalidConfig {
            reason: "backend must select exactly one database backend".to_string(),
        }),
        1 => Ok(selected.remove(0).1),
        _ => {
            let names = selected.into_iter().map(|(name, _)| name).collect::<Vec<_>>().join(", ");
            Err(RegistryError::InvalidConfig {
                reason: format!("backend must select exactly one database backend, got {names}"),
            })
        }
    }
}

fn build_sql_backend_settings(
    backend: &str,
    file: SqlBackendFile,
) -> Result<SqlBackendSettings, RegistryError> {
    let timeout = parse_backend_interval(backend, "timeout", file.timeout.as_ref())?
        .unwrap_or(SqlBackendSettings::DEFAULT_TIMEOUT);
    if timeout.is_zero() {
        return Err(RegistryError::InvalidConfig {
            reason: format!("backend.{backend}.timeout must be greater than 0"),
        });
    }
    let startup_timeout =
        parse_backend_interval(backend, "startupTimeout", file.startup_timeout.as_ref())?
            .unwrap_or(SqlBackendSettings::DEFAULT_STARTUP_TIMEOUT);
    if startup_timeout.is_zero() {
        return Err(RegistryError::InvalidConfig {
            reason: format!("backend.{backend}.startupTimeout must be greater than 0"),
        });
    }
    Ok(SqlBackendSettings {
        url: file.url,
        max_connections: file.max_connections,
        timeout,
        startup_timeout,
    })
}

fn parse_backend_interval(
    backend: &str,
    field: &str,
    raw: Option<&Interval>,
) -> Result<Option<Duration>, RegistryError> {
    raw.map(|Interval(value)| {
        parse_interval(value).ok_or_else(|| RegistryError::InvalidConfig {
            reason: format!("backend.{backend}.{field} has an invalid interval {value:?}"),
        })
    })
    .transpose()
}

fn resolve_libsql_paths(settings: &mut LibsqlSettings, base_dir: &Path) {
    // Resolve a relative `replicaPath` against the config file's
    // directory, the same convention `storage` and the auth files
    // follow, so `./auth-replica.db` lands next to the config rather
    // than in the process CWD.
    if let Some(path) = settings.replica_path.take() {
        settings.replica_path = Some(if path.is_absolute() { path } else { base_dir.join(path) });
    }
}

/// Lift the YAML `log:` object's `format` / `level` onto runtime
/// defaults. Missing block = default pretty/info config; missing
/// individual fields fall back to their `Default` impls.
fn build_log_config(entry: Option<&LogEntryFile>) -> LogConfig {
    let Some(entry) = entry else { return LogConfig::default() };
    LogConfig {
        format: entry.format.unwrap_or_default(),
        level: entry.level.unwrap_or_default(),
        sink: entry.r#type.clone(),
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
