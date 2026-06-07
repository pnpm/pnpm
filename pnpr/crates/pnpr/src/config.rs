use crate::{
    error::RegistryError,
    policy::{AccessList, PackagePolicies, PackagePolicy},
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
/// (uplinks, package routing) without reading a file from disk —
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
/// `storage`, `uplinks`, `packages` — restricted to the subset
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
    /// Named upstream npm registries. Referenced by name from
    /// [`PackageAccess::proxy`].
    pub uplinks: IndexMap<String, UplinkConfig>,
    /// Package routing rules, evaluated in declared order. The first
    /// pattern that matches a requested package supplies its
    /// uplink (via `proxy`). Patterns without a `proxy` make the
    /// package storage-only (effectively static for that pattern).
    pub packages: IndexMap<String, PackageAccess>,
    /// How long a cached packument is considered fresh before it is
    /// re-fetched from the resolved uplink. Ignored when no uplink
    /// matches.
    pub packument_ttl: Duration,
    /// Per-package access and publish rules. [`Config::from_yaml`]
    /// compiles these from the YAML `packages:` block (each entry's
    /// `access` / `publish` tokens); the programmatic
    /// [`Config::proxy`] / [`Config::static_serve`] constructors use
    /// [`PackagePolicies::registry_mock_defaults`] instead, enforcing
    /// the `@private/*` and `@pnpm.e2e/needs-auth` rules
    /// `@pnpm/registry-mock` applied under verdaccio.
    pub policies: PackagePolicies,
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
    /// plus `SQLite` token database. The YAML `backend.libsql:` block
    /// switches both to a shared networked-SQLite database so several
    /// stateless pnpr replicas see a consistent set of accounts.
    pub backend: BackendConfig,
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
/// fallible step (connecting to the networked database and ensuring its
/// schema) is async, so it runs in `AuthState::load` rather than at
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
/// * absent → unlimited (verdaccio's `+infinity` default; the YAML
///   `+inf` token is a float literal and won't parse into the
///   `i64` field, so the only way to ask for "no cap" is to omit
///   the key)
/// * `-1` → registration disabled
/// * non-negative `n` → at most `n` users
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum MaxUsers {
    #[default]
    Unlimited,
    Disabled,
    Limited(u64),
}

impl MaxUsers {
    /// Translate the YAML value into [`MaxUsers`]. Verdaccio accepts
    /// any signed integer here; negative anything other than `-1` is
    /// nonsense and is treated as "disabled" to err on the side of
    /// rejecting unsafe configs.
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

/// Runtime uplink declaration: the upstream `url` plus the request
/// headers pnpr attaches to every fetch it makes to that uplink.
///
/// [`Self::headers`] is resolved once, at config load, from the YAML
/// `auth:` block (an `Authorization` header derived from
/// `type`/`token`/`token_env`) merged with the `headers:` map. The
/// parse-time shape lives in `UplinkFile`; `resolve_uplink` turns
/// one into the other. Verdaccio fields pnpr doesn't model yet
/// (timeouts, agent options, `maxage`) are accepted and dropped.
#[derive(Clone)]
pub struct UplinkConfig {
    pub url: String,
    /// Auth + custom headers, fully resolved and ready to attach to
    /// every request pnpr makes to this uplink.
    pub headers: HeaderMap,
}

impl fmt::Debug for UplinkConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UplinkConfig")
            .field("url", &self.url)
            .field("headers", &RedactedHeaders(&self.headers))
            .finish()
    }
}

/// Wraps a [`HeaderMap`] so its `Debug` lists header names with values
/// redacted. Uplink headers carry credentials (an `Authorization`, or
/// an API key in a custom header), and those must never reach a log
/// line, span, or diagnostic dump.
pub(crate) struct RedactedHeaders<'a>(pub(crate) &'a HeaderMap);

impl fmt::Debug for RedactedHeaders<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.0.keys().map(|name| (name.as_str(), "<redacted>"))).finish()
    }
}

/// Disk shape of one `uplinks:` entry. Mirrors verdaccio's uplink
/// config for the subset pnpr implements: `url`, an `auth:` block,
/// and a free-form `headers:` map. Resolved into [`UplinkConfig`] by
/// [`resolve_uplink`].
#[derive(Debug, Deserialize)]
struct UplinkFile {
    url: String,
    #[serde(default)]
    auth: Option<UplinkAuthFile>,
    #[serde(default)]
    headers: IndexMap<String, String>,
}

/// The YAML `auth:` block on an uplink. `token` takes priority over
/// `token_env`; either resolves to the credential placed in the
/// `Authorization` header, encoded per [`UplinkAuthType`].
#[derive(Debug, Deserialize)]
struct UplinkAuthFile {
    r#type: UplinkAuthType,
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
enum UplinkAuthType {
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

/// Resolve one parsed [`UplinkFile`] into a runtime [`UplinkConfig`],
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
fn resolve_uplink<Sys: EnvVar>(
    name: &str,
    file: UplinkFile,
) -> Result<UplinkConfig, RegistryError> {
    let mut headers = HeaderMap::new();
    if let Some(auth) = &file.auth {
        let token =
            resolve_uplink_token::<Sys>(auth).ok_or_else(|| RegistryError::InvalidConfig {
                reason: format!(
                    "uplink {name:?} has an auth block but no token could be resolved \
                     (set auth.token or point auth.token_env at a set env var)",
                ),
            })?;
        let value = match auth.r#type {
            UplinkAuthType::Bearer => format!("Bearer {token}"),
            UplinkAuthType::Basic => format!("Basic {token}"),
        };
        let value = HeaderValue::from_str(&value).map_err(|_| RegistryError::InvalidConfig {
            reason: format!("uplink {name:?} auth token is not a valid header value"),
        })?;
        headers.insert(AUTHORIZATION, value);
    }
    for (raw_name, raw_value) in &file.headers {
        let header_name = HeaderName::from_bytes(raw_name.as_bytes()).map_err(|_| {
            RegistryError::InvalidConfig {
                reason: format!("uplink {name:?} has an invalid header name {raw_name:?}"),
            }
        })?;
        let header_value =
            HeaderValue::from_str(raw_value).map_err(|_| RegistryError::InvalidConfig {
                reason: format!("uplink {name:?} header {raw_name:?} has an invalid value"),
            })?;
        headers.insert(header_name, header_value);
    }
    Ok(UplinkConfig { url: file.url, headers })
}

/// Pick the credential for an uplink's `auth:` block: an explicit
/// `token` wins; otherwise read the env var named by `token_env`.
fn resolve_uplink_token<Sys: EnvVar>(auth: &UplinkAuthFile) -> Option<String> {
    if let Some(token) = &auth.token {
        return non_empty_token(token);
    }
    let var_name = auth.token_env.as_ref()?.var_name()?;
    Sys::var(var_name).and_then(|token| non_empty_token(&token))
}

fn non_empty_token(token: &str) -> Option<String> {
    (!token.trim().is_empty()).then(|| token.to_string())
}

/// Per-package routing and access rules. `access` / `publish` are
/// verdaccio permission lists (built-in groups like `$all` /
/// `$authenticated` / `$anonymous`, plus usernames / group names),
/// compiled into the [`PackagePolicies`] that gate reads and writes.
/// `unpublish` is parsed but currently folded into `publish` at
/// enforcement time. `proxy` selects the [`UplinkConfig`] by name.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct PackageAccess {
    pub access: Option<AccessSpec>,
    pub publish: Option<AccessSpec>,
    pub unpublish: Option<AccessSpec>,
    pub proxy: Option<String>,
}

/// A YAML permission value. Verdaccio accepts either a single
/// space-separated string (`access: $authenticated admin`) or a
/// sequence (`access: [$authenticated, admin]`); both normalize to the
/// same token list.
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
    /// with a networked `SQLite` database. Absent on a stock verdaccio
    /// config (silently ignored there).
    #[serde(default)]
    backend: Option<BackendFile>,
    #[serde(default)]
    uplinks: IndexMap<String, UplinkFile>,
    #[serde(default)]
    packages: IndexMap<String, PackageAccess>,
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

impl Config {
    /// Default `listen` when one isn't supplied by the caller.
    pub const DEFAULT_LISTEN: &'static str = "127.0.0.1:4873";
    /// Default packument TTL — five minutes, matching the historical
    /// proxy-mode default.
    pub const DEFAULT_PACKUMENT_TTL: Duration = Duration::from_mins(5);

    /// Build a proxy-mode config with the default npm upstream: a single
    /// `npmjs` uplink plus a `**` package rule that routes everything
    /// through it. Kept for callers that don't use YAML config.
    #[must_use]
    pub fn proxy(listen: SocketAddr, storage: PathBuf) -> Self {
        let mut uplinks = IndexMap::new();
        uplinks.insert(
            "npmjs".to_string(),
            UplinkConfig {
                url: "https://registry.npmjs.org".to_string(),
                headers: HeaderMap::new(),
            },
        );
        let mut packages = IndexMap::new();
        packages.insert(
            "**".to_string(),
            PackageAccess { proxy: Some("npmjs".to_string()), ..Default::default() },
        );
        Self {
            listen,
            public_url: format!("http://{listen}"),
            cache_storage: default_cache_dir(&storage),
            storage,
            uplinks,
            packages,
            packument_ttl: Self::DEFAULT_PACKUMENT_TTL,
            policies: PackagePolicies::registry_mock_defaults(),
            auth: AuthConfig::default(),
            logs: LogConfig::default(),
            hosted_store: HostedStoreConfig::Fs,
            backend: BackendConfig::Local,
        }
    }

    /// Build a static-mode config that serves `storage` verbatim:
    /// no uplinks declared, so no package rule resolves to one.
    #[must_use]
    pub fn static_serve(listen: SocketAddr, storage: PathBuf) -> Self {
        Self {
            listen,
            public_url: format!("http://{listen}"),
            cache_storage: default_cache_dir(&storage),
            storage,
            uplinks: IndexMap::new(),
            packages: IndexMap::new(),
            packument_ttl: Self::DEFAULT_PACKUMENT_TTL,
            policies: PackagePolicies::registry_mock_defaults(),
            auth: AuthConfig::default(),
            logs: LogConfig::default(),
            hosted_store: HostedStoreConfig::Fs,
            backend: BackendConfig::Local,
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
        let raw = std::fs::read_to_string(path).map_err(|err| {
            std::io::Error::new(err.kind(), format!("read {}: {err}", path.display()))
        })?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        Self::from_yaml_str(&raw, base, listen, public_url).map_err(|err| {
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
        Self::from_yaml_str(DEFAULT_CONFIG_YAML, base_dir, listen, public_url)
            .expect("bundled DEFAULT_CONFIG_YAML must always parse")
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
        if let Some(path) = explicit {
            let config = Self::from_yaml(path, listen, public_url)?;
            return Ok((config, ConfigSource::Cli(path.to_path_buf())));
        }
        if let Some(path) = default_path {
            let config = Self::from_yaml(path, listen, public_url)?;
            return Ok((config, ConfigSource::DefaultPath(path.to_path_buf())));
        }
        Ok((Self::from_default_yaml(Path::new("."), listen, public_url), ConfigSource::Bundled))
    }

    fn from_yaml_str(
        raw: &str,
        base_dir: &Path,
        listen: SocketAddr,
        public_url: Option<String>,
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
        let backend = match file.backend.and_then(|block| block.libsql) {
            Some(mut settings) => {
                // Resolve a relative `replicaPath` against the config
                // file's directory, the same convention `storage` and
                // the auth files follow, so `./auth-replica.db` lands
                // next to the config rather than in the process CWD.
                if let Some(path) = settings.replica_path.take() {
                    settings.replica_path =
                        Some(if path.is_absolute() { path } else { base_dir.join(path) });
                }
                BackendConfig::Libsql(settings)
            }
            None => BackendConfig::Local,
        };
        let public_url = public_url.unwrap_or_else(|| format!("http://{listen}"));
        let auth = build_auth_config(&file.auth, base_dir);
        let logs = build_log_config(file.log.as_ref());
        let policies = build_policies(&file.packages)?;
        let uplinks = file
            .uplinks
            .into_iter()
            .map(|(name, uplink)| {
                let resolved = resolve_uplink::<SystemEnv>(&name, uplink)?;
                Ok((name, resolved))
            })
            .collect::<Result<IndexMap<_, _>, RegistryError>>()?;
        Ok(Self {
            listen,
            public_url,
            storage,
            cache_storage,
            uplinks,
            packages: file.packages,
            packument_ttl: Self::DEFAULT_PACKUMENT_TTL,
            policies,
            auth,
            logs,
            hosted_store,
            backend,
        })
    }

    /// Find the uplink for `package_name` by walking [`Self::packages`]
    /// in declared order: the first pattern that matches is the rule
    /// that applies. If that rule has no `proxy:`, the package is
    /// storage-only and this returns `None` — matching verdaccio's
    /// first-match-wins semantics. The returned tuple's first element
    /// is the uplink *name* (the key in [`Self::uplinks`]); callers
    /// that have pre-built per-uplink state can use it as an index.
    #[must_use]
    pub fn resolve_uplink(&self, package_name: &str) -> Option<(&str, &UplinkConfig)> {
        let access = self.packages.iter().find_map(|(pattern, access)| {
            pattern_matches(pattern, package_name).then_some(access)
        })?;
        let proxy_name = access.proxy.as_deref()?;
        self.uplinks.get_key_value(proxy_name).map(|(k, v)| (k.as_str(), v))
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
            max_users: file.htpasswd.max_users.map_or(MaxUsers::Unlimited, MaxUsers::from_yaml),
        },
        tokens: TokensConfig { file: tokens_file },
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

/// Compile the YAML `packages:` rules into the runtime
/// [`PackagePolicies`], in declared order (first match wins). A
/// missing `access` defaults to `$all`, a missing `publish` to
/// `$authenticated` — the same safe fallback [`PackagePolicies`]
/// applies to packages no rule matches. `unpublish` is parsed for
/// config compatibility but not yet enforced separately (it folds
/// into `publish`). Errors only on an invalid glob pattern — any
/// token string is a valid group/username, as in verdaccio.
fn build_policies(
    packages: &IndexMap<String, PackageAccess>,
) -> Result<PackagePolicies, RegistryError> {
    let rules = packages
        .iter()
        .map(|(pattern, access)| {
            let access_list = access
                .access
                .as_ref()
                .map_or_else(|| AccessList::parse("$all"), AccessSpec::to_access_list);
            let publish_list = access
                .publish
                .as_ref()
                .map_or_else(|| AccessList::parse("$authenticated"), AccessSpec::to_access_list);
            PackagePolicy::new(pattern, access_list, publish_list)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PackagePolicies::new(rules))
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

/// Match a verdaccio package pattern against a package name.
/// Supports:
///   - `**`        — matches everything
///   - `@*/*`      — matches all scoped packages
///   - `@scope/*`  — matches every package in a specific scope
///   - exact name  — literal match
fn pattern_matches(pattern: &str, name: &str) -> bool {
    if pattern == "**" {
        return true;
    }
    if pattern == "@*/*" {
        return name.starts_with('@');
    }
    if let Some(scope) = pattern.strip_suffix("/*").and_then(|p| p.strip_prefix('@')) {
        let Some(name_scope) = name.strip_prefix('@').and_then(|n| n.split('/').next()) else {
            return false;
        };
        return name_scope == scope;
    }
    pattern == name
}

#[cfg(test)]
mod tests;
