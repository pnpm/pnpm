use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use indexmap::IndexMap;
use pacquet_env_replace::{SystemEnv, env_replace_lossy};
use serde::Deserialize;

use crate::error::RegistryError;
use crate::policy::{AccessList, PackagePolicies, PackagePolicy};

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
    /// Directory under which packuments and tarballs live.
    /// In proxy mode this doubles as the cache; with no matching
    /// `proxy:` rule it is the source of truth.
    pub storage: PathBuf,
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

/// Verdaccio-shaped uplink declaration. Only `url` is honored —
/// other fields verdaccio supports (auth headers, timeouts, agent
/// options) are not implemented yet.
#[derive(Debug, Clone, Deserialize)]
pub struct UplinkConfig {
    pub url: String,
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
    #[serde(default)]
    uplinks: IndexMap<String, UplinkConfig>,
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
    pub const DEFAULT_PACKUMENT_TTL: Duration = Duration::from_secs(5 * 60);

    /// Build a proxy-mode config with the default npm upstream: a single
    /// `npmjs` uplink plus a `**` package rule that routes everything
    /// through it. Kept for callers that don't use YAML config.
    pub fn proxy(listen: SocketAddr, storage: PathBuf) -> Self {
        let mut uplinks = IndexMap::new();
        uplinks.insert(
            "npmjs".to_string(),
            UplinkConfig { url: "https://registry.npmjs.org".to_string() },
        );
        let mut packages = IndexMap::new();
        packages.insert(
            "**".to_string(),
            PackageAccess { proxy: Some("npmjs".to_string()), ..Default::default() },
        );
        Self {
            listen,
            public_url: format!("http://{listen}"),
            storage,
            uplinks,
            packages,
            packument_ttl: Self::DEFAULT_PACKUMENT_TTL,
            policies: PackagePolicies::registry_mock_defaults(),
            auth: AuthConfig::default(),
            logs: LogConfig::default(),
        }
    }

    /// Build a static-mode config that serves `storage` verbatim:
    /// no uplinks declared, so no package rule resolves to one.
    pub fn static_serve(listen: SocketAddr, storage: PathBuf) -> Self {
        Self {
            listen,
            public_url: format!("http://{listen}"),
            storage,
            uplinks: IndexMap::new(),
            packages: IndexMap::new(),
            packument_ttl: Self::DEFAULT_PACKUMENT_TTL,
            policies: PackagePolicies::registry_mock_defaults(),
            auth: AuthConfig::default(),
            logs: LogConfig::default(),
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
        let public_url = public_url.unwrap_or_else(|| format!("http://{listen}"));
        let auth = build_auth_config(&file.auth, base_dir);
        let logs = build_log_config(file.log.as_ref());
        let policies = build_policies(&file.packages)?;
        Ok(Self {
            listen,
            public_url,
            storage,
            uplinks: file.uplinks,
            packages: file.packages,
            packument_ttl: Self::DEFAULT_PACKUMENT_TTL,
            policies,
            auth,
            logs,
        })
    }

    /// Find the uplink for `package_name` by walking [`Self::packages`]
    /// in declared order: the first pattern that matches is the rule
    /// that applies. If that rule has no `proxy:`, the package is
    /// storage-only and this returns `None` — matching verdaccio's
    /// first-match-wins semantics. The returned tuple's first element
    /// is the uplink *name* (the key in [`Self::uplinks`]); callers
    /// that have pre-built per-uplink state can use it as an index.
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
mod tests {
    use super::{
        Config, ConfigSource, DEFAULT_CONFIG_YAML, LogFormat, LogLevel, config_file_in,
        pattern_matches, resolve_relative,
    };
    use crate::policy::Identity;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::path::{Path, PathBuf};

    fn user(name: &str) -> Identity {
        Identity::User { username: name.to_string() }
    }

    fn listen() -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873))
    }

    #[test]
    fn pattern_double_star_matches_anything() {
        assert!(pattern_matches("**", "lodash"));
        assert!(pattern_matches("**", "@foo/bar"));
        assert!(pattern_matches("**", ""));
    }

    #[test]
    fn pattern_any_scope_matches_only_scoped() {
        assert!(pattern_matches("@*/*", "@foo/bar"));
        assert!(pattern_matches("@*/*", "@pnpm.e2e/needs-auth"));
        assert!(!pattern_matches("@*/*", "lodash"));
    }

    #[test]
    fn pattern_specific_scope_matches_only_that_scope() {
        assert!(pattern_matches("@private/*", "@private/anything"));
        assert!(!pattern_matches("@private/*", "@public/anything"));
        assert!(!pattern_matches("@private/*", "private"));
    }

    #[test]
    fn pattern_exact_match() {
        assert!(pattern_matches("foobar", "foobar"));
        assert!(!pattern_matches("foobar", "foobaz"));
        assert!(!pattern_matches("foobar", "@scope/foobar"));
    }

    #[test]
    fn resolve_relative_passes_absolute_paths_through() {
        let absolute = PathBuf::from("/tmp/storage");
        assert_eq!(resolve_relative("/tmp/storage", Path::new("/anywhere")), absolute);
    }

    #[test]
    fn resolve_relative_joins_relative_paths_to_base() {
        assert_eq!(
            resolve_relative("./storage", Path::new("/etc/pnpr")),
            PathBuf::from("/etc/pnpr/./storage"),
        );
    }

    #[test]
    fn proxy_constructor_routes_everything_through_npmjs() {
        let config = Config::proxy(listen(), PathBuf::from("/tmp"));
        let (name, uplink) = config.resolve_uplink("anything").expect("** rule matches");
        assert_eq!(name, "npmjs");
        assert_eq!(uplink.url, "https://registry.npmjs.org");
    }

    #[test]
    fn static_constructor_has_no_uplinks() {
        let config = Config::static_serve(listen(), PathBuf::from("/tmp"));
        assert!(config.uplinks.is_empty());
        assert!(config.packages.is_empty());
        assert!(config.resolve_uplink("anything").is_none());
    }

    #[test]
    fn from_default_yaml_parses_bundled_file() {
        let config = Config::from_default_yaml(Path::new("/tmp"), listen(), None);
        assert!(config.uplinks.contains_key("npmjs"));
        assert_eq!(config.uplinks["npmjs"].url, "https://registry.npmjs.org/");
        // The bundled file routes the catch-all through npmjs.
        let (name, _) = config.resolve_uplink("lodash").expect("** -> npmjs in defaults");
        assert_eq!(name, "npmjs");
    }

    #[test]
    fn default_yaml_const_matches_what_from_default_parses() {
        // Sanity check: the const is non-empty and round-trips through
        // the parser without panicking — i.e. `from_default_yaml`'s
        // `expect(...)` is not a tripwire under future edits.
        assert!(!DEFAULT_CONFIG_YAML.is_empty());
        let _ = Config::from_default_yaml(Path::new("."), listen(), None);
    }

    #[test]
    fn from_yaml_str_storage_is_resolved_relative_to_base_dir() {
        let yaml = "storage: ./store\nuplinks: {}\npackages: {}\n";
        let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
        assert_eq!(config.storage, PathBuf::from("/etc/pnpr/./store"));
    }

    #[test]
    fn from_yaml_str_absolute_storage_is_left_alone() {
        let yaml = "storage: /var/lib/pnpr\nuplinks: {}\npackages: {}\n";
        let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
        assert_eq!(config.storage, PathBuf::from("/var/lib/pnpr"));
    }

    #[test]
    fn from_yaml_str_ignores_unknown_sections() {
        // Sections we don't implement (`auth`, `web`, `plugins`, etc.)
        // must parse silently so existing config files work untouched.
        let yaml = "\
storage: ./s
auth:
  htpasswd:
    file: ./htpasswd
web:
  enable: false
plugins: ../node_modules
secret: hunter2
uplinks:
  npmjs:
    url: https://registry.npmjs.org/
packages:
  '**':
    access: $all
    proxy: npmjs
";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        let (name, uplink) = config.resolve_uplink("anything").expect("** -> npmjs");
        assert_eq!(name, "npmjs");
        assert_eq!(uplink.url, "https://registry.npmjs.org/");
    }

    #[test]
    fn from_yaml_str_packages_evaluated_in_declared_order() {
        // First match wins: `@private/*` should resolve before `**`
        // even though both are declared.
        let yaml = "\
storage: ./s
uplinks:
  mirror: { url: https://mirror.example/ }
  npmjs:  { url: https://registry.npmjs.org/ }
packages:
  '@private/*':
    proxy: mirror
  '**':
    proxy: npmjs
";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        assert_eq!(config.resolve_uplink("@private/foo").unwrap().0, "mirror");
        assert_eq!(config.resolve_uplink("lodash").unwrap().0, "npmjs");
    }

    #[test]
    fn from_yaml_str_package_without_proxy_does_not_resolve_an_uplink() {
        // Verdaccio first-match-wins: a pattern entry that matches but
        // has no `proxy:` is storage-only — resolution stops there and
        // returns None instead of falling through to a later catch-all.
        let yaml = "\
storage: ./s
uplinks:
  npmjs: { url: https://registry.npmjs.org/ }
packages:
  '@private/*':
    access: $authenticated
  '**':
    proxy: npmjs
";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        assert!(config.resolve_uplink("@private/foo").is_none());
        // Unrelated names still fall through to `**` -> `npmjs`.
        assert_eq!(config.resolve_uplink("lodash").unwrap().0, "npmjs");
    }

    #[test]
    fn from_yaml_str_public_url_defaults_to_listen_when_none_passed() {
        let yaml = "storage: ./s\nuplinks: {}\npackages: {}\n";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        assert_eq!(config.public_url, format!("http://{}", listen()));
    }

    #[test]
    fn from_yaml_str_public_url_override_wins() {
        let yaml = "storage: ./s\nuplinks: {}\npackages: {}\n";
        let config = Config::from_yaml_str(
            yaml,
            Path::new("/x"),
            listen(),
            Some("http://override.test".to_string()),
        )
        .unwrap();
        assert_eq!(config.public_url, "http://override.test");
    }

    #[test]
    fn from_yaml_path_round_trips_through_tempfile() {
        // Exercise the file-reading path (not just the in-memory
        // `from_yaml_str` shortcut). Confirms relative `storage:` is
        // resolved against the *config file's* parent dir.
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("registry.yml");
        std::fs::write(&config_path, "storage: ./store\nuplinks: {}\npackages: {}\n").unwrap();
        let config = Config::from_yaml(&config_path, listen(), None).unwrap();
        assert_eq!(config.storage, dir.path().join("./store"));
    }

    #[test]
    fn from_yaml_path_surfaces_parse_errors_as_invalid_data() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("broken.yml");
        std::fs::write(&config_path, "storage: [not, a, string\n").unwrap();
        let err = Config::from_yaml(&config_path, listen(), None).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn from_yaml_path_propagates_missing_file_errors() {
        let err = Config::from_yaml(Path::new("/no/such/file.yml"), listen(), None).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn auth_block_resolves_htpasswd_relative_to_config_dir() {
        let yaml = "\
storage: ./s
auth:
  htpasswd:
    file: ./htpasswd
uplinks: {}
packages: {}
";
        let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
        assert_eq!(config.auth.htpasswd.file.as_deref(), Some(Path::new("/etc/pnpr/./htpasswd")));
        // Tokens default to the htpasswd sibling.
        assert_eq!(config.auth.tokens.file.as_deref(), Some(Path::new("/etc/pnpr/tokens.db")));
    }

    #[test]
    fn auth_block_absent_keeps_in_memory_defaults() {
        let yaml = "storage: ./s\nuplinks: {}\npackages: {}\n";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        assert!(config.auth.htpasswd.file.is_none());
        assert!(config.auth.tokens.file.is_none());
        assert_eq!(config.auth.htpasswd.max_users, super::MaxUsers::Unlimited);
    }

    #[test]
    fn auth_tokens_file_explicit_override_wins_over_sibling_default() {
        let yaml = "\
storage: ./s
auth:
  htpasswd:
    file: ./htpasswd
  tokens:
    file: /var/lib/pnpr/tokens.sqlite
uplinks: {}
packages: {}
";
        let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
        assert_eq!(
            config.auth.tokens.file.as_deref(),
            Some(Path::new("/var/lib/pnpr/tokens.sqlite")),
        );
    }

    #[test]
    fn auth_max_users_negative_one_means_disabled() {
        let yaml = "\
storage: ./s
auth:
  htpasswd:
    file: ./htpasswd
    max_users: -1
uplinks: {}
packages: {}
";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        assert_eq!(config.auth.htpasswd.max_users, super::MaxUsers::Disabled);
    }

    #[test]
    fn auth_max_users_positive_is_a_hard_cap() {
        let yaml = "\
storage: ./s
auth:
  htpasswd:
    file: ./htpasswd
    max_users: 5
uplinks: {}
packages: {}
";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        assert_eq!(config.auth.htpasswd.max_users, super::MaxUsers::Limited(5));
    }

    #[test]
    fn logs_default_when_yaml_omits_block() {
        let yaml = "storage: ./s\nuplinks: {}\npackages: {}\n";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        assert_eq!(config.logs.format, LogFormat::Pretty);
        assert_eq!(config.logs.level, LogLevel::Info);
        assert_eq!(config.logs.sink, "stdout");
        assert!(config.logs.sink_is_supported());
    }

    #[test]
    fn log_unsupported_sink_type_is_recorded_but_flagged_unsupported() {
        // `type: file` parses (verdaccio compatibility) but is not a
        // sink the server implements, so `sink_is_supported` is false
        // and the binary warns at startup. Format/level still apply.
        let yaml = "\
storage: ./s
uplinks: {}
packages: {}
log:
  type: file
  format: json
";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        assert_eq!(config.logs.sink, "file");
        assert!(!config.logs.sink_is_supported());
        assert_eq!(config.logs.format, LogFormat::Json);
    }

    #[test]
    fn log_pretty_and_level_picked_from_singular_block() {
        let yaml = "\
storage: ./s
uplinks: {}
packages: {}
log:
  type: stdout
  format: pretty
  level: warn
";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        assert_eq!(config.logs.format, LogFormat::Pretty);
        assert_eq!(config.logs.level, LogLevel::Warn);
    }

    #[test]
    fn log_json_format_parses() {
        let yaml = "\
storage: ./s
uplinks: {}
packages: {}
log:
  type: stdout
  format: json
  level: debug
";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        assert_eq!(config.logs.format, LogFormat::Json);
        assert_eq!(config.logs.level, LogLevel::Debug);
    }

    #[test]
    fn log_legacy_plural_list_is_ignored() {
        // Verdaccio 4/5 used `logs:` as a list. We only honor the
        // verdaccio-6 `log:` (singular) shape, so the older spelling
        // is silently dropped and defaults apply.
        let yaml = "\
storage: ./s
uplinks: {}
packages: {}
logs:
  - type: stdout
    format: json
    level: error
";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        assert_eq!(config.logs.format, LogFormat::Pretty);
        assert_eq!(config.logs.level, LogLevel::Info);
    }

    #[test]
    fn log_missing_fields_fall_back_to_defaults() {
        // Only `type:` is given. Format and level default individually.
        let yaml = "\
storage: ./s
uplinks: {}
packages: {}
log:
  type: stdout
";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        assert_eq!(config.logs.format, LogFormat::Pretty);
        assert_eq!(config.logs.level, LogLevel::Info);
    }

    #[test]
    fn log_level_filter_directives_are_valid() {
        // Each LogLevel must map to a directive string that
        // `EnvFilter::new` accepts at runtime — guards against typos.
        for level in [
            LogLevel::Trace,
            LogLevel::Debug,
            LogLevel::Http,
            LogLevel::Info,
            LogLevel::Warn,
            LogLevel::Error,
        ] {
            let directive = level.as_filter_directive();
            tracing_subscriber::EnvFilter::try_new(directive)
                .unwrap_or_else(|err| panic!("{level:?} -> `{directive}`: {err}"));
        }
    }

    // ----- config_file_in (existence gating) --------------------------------

    #[test]
    fn config_file_in_returns_none_for_none_dir() {
        assert!(config_file_in(None).is_none());
    }

    #[test]
    fn config_file_in_returns_none_when_file_is_missing() {
        // A fresh tempdir has no `config.yaml`.
        let dir = tempfile::tempdir().unwrap();
        assert!(config_file_in(Some(dir.path().to_path_buf())).is_none());
    }

    #[test]
    fn config_file_in_returns_path_when_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let expected = dir.path().join("config.yaml");
        std::fs::write(&expected, "storage: ./s\nuplinks: {}\npackages: {}\n").unwrap();
        let resolved = config_file_in(Some(dir.path().to_path_buf())).expect("file is present");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn config_file_in_rejects_a_directory_at_the_target() {
        // If `config.yaml` exists but is a directory (or symlink to
        // one, etc.), `is_file()` returns false. Auto-discovery should
        // bail rather than try to read it.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("config.yaml")).unwrap();
        assert!(config_file_in(Some(dir.path().to_path_buf())).is_none());
    }

    #[test]
    fn config_file_in_resolved_file_round_trips_through_from_yaml() {
        // The whole point of returning a path is that `from_yaml` can
        // load it. This is the end-to-end happy path for the
        // auto-discovery flow.
        //
        // The `storage:` value is computed at runtime so it's a
        // genuinely absolute path on whichever OS the test runs on
        // (Windows requires a drive-letter prefix to satisfy
        // `Path::is_absolute()`; a Unix-style "/tmp/auto" is not
        // absolute there and gets joined to the config's parent dir).
        let dir = tempfile::tempdir().unwrap();
        let storage = dir.path().join("registry-storage");
        let yaml = format!(
            "\
storage: {storage}
uplinks:
  npmjs: {{ url: https://registry.npmjs.org/ }}
packages:
  '**':
    proxy: npmjs
log:
  type: stdout
  format: json
  level: info
",
            storage = storage.display(),
        );
        std::fs::write(dir.path().join("config.yaml"), yaml).unwrap();
        let path = config_file_in(Some(dir.path().to_path_buf())).unwrap();
        let config = Config::from_yaml(&path, listen(), None).unwrap();
        assert_eq!(config.storage, storage);
        assert_eq!(config.logs.format, LogFormat::Json);
        assert_eq!(config.logs.level, LogLevel::Info);
        assert_eq!(config.resolve_uplink("lodash").unwrap().0, "npmjs");
    }

    // ----- LogFormat / LogLevel serde behavior ------------------------------

    /// Helper: deserialize a YAML scalar into the requested enum.
    /// Lets us assert the variant mapping concisely.
    fn parse_log_yaml<Target: serde::de::DeserializeOwned>(yaml: &str) -> Result<Target, String> {
        serde_saphyr::from_str::<Target>(yaml).map_err(|err| err.to_string())
    }

    #[test]
    fn log_format_accepts_each_known_variant() {
        assert_eq!(parse_log_yaml::<LogFormat>("pretty").unwrap(), LogFormat::Pretty);
        assert_eq!(parse_log_yaml::<LogFormat>("json").unwrap(), LogFormat::Json);
    }

    #[test]
    fn log_format_rejects_unknown_variant() {
        // `format: xml` (or anything else) should fail parsing
        // rather than silently fall back. Matches verdaccio: an
        // unknown enum value is a typo, not a request for a default.
        let err = parse_log_yaml::<LogFormat>("xml").unwrap_err();
        assert!(err.contains("xml") || err.to_lowercase().contains("unknown"));
    }

    #[test]
    fn log_format_is_case_sensitive() {
        // `rename_all = "lowercase"` means we accept only lowercase
        // tokens; pino is case-sensitive too.
        assert!(parse_log_yaml::<LogFormat>("Pretty").is_err());
        assert!(parse_log_yaml::<LogFormat>("JSON").is_err());
    }

    #[test]
    fn log_level_accepts_each_known_variant() {
        let pairs: &[(&str, LogLevel)] = &[
            ("trace", LogLevel::Trace),
            ("debug", LogLevel::Debug),
            ("http", LogLevel::Http),
            ("info", LogLevel::Info),
            ("warn", LogLevel::Warn),
            ("error", LogLevel::Error),
        ];
        for (yaml, expected) in pairs {
            let parsed: LogLevel = parse_log_yaml(yaml).unwrap();
            assert_eq!(parsed, *expected, "{yaml}");
        }
    }

    #[test]
    fn log_level_rejects_unknown_variant() {
        // `fatal` (pino has it) and `silly` (npm's logger had it)
        // are not in our set — we want a hard error, not a silent
        // fallback.
        assert!(parse_log_yaml::<LogLevel>("fatal").is_err());
        assert!(parse_log_yaml::<LogLevel>("silly").is_err());
        assert!(parse_log_yaml::<LogLevel>("verbose").is_err());
    }

    // ----- Config::resolve precedence ---------------------------------------

    /// Helper: write a config file under a tempdir and hand back the
    /// path. Tests use this to populate both the explicit `-c` arg
    /// and the auto-discovered default path.
    fn write_yaml(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).expect("write yaml fixture");
        path
    }

    const MINIMAL_YAML: &str = "storage: ./s\nuplinks: {}\npackages: {}\n";

    #[test]
    fn resolve_bundled_when_no_path_supplied() {
        let (config, source) = Config::resolve(None, None, listen(), None).unwrap();
        assert_eq!(source, ConfigSource::Bundled);
        // The bundled config has the `npmjs` uplink + `**` route.
        assert!(config.uplinks.contains_key("npmjs"));
    }

    #[test]
    fn resolve_default_path_when_only_default_supplied() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_yaml(tmp.path(), "config.yaml", MINIMAL_YAML);
        let (_, source) = Config::resolve(None, Some(&path), listen(), None).unwrap();
        assert_eq!(source, ConfigSource::DefaultPath(path));
    }

    #[test]
    fn resolve_cli_when_only_cli_supplied() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_yaml(tmp.path(), "explicit.yml", MINIMAL_YAML);
        let (_, source) = Config::resolve(Some(&path), None, listen(), None).unwrap();
        assert_eq!(source, ConfigSource::Cli(path));
    }

    #[test]
    fn resolve_cli_wins_over_default_path() {
        // Both paths exist. CLI must take priority — the auto-discovered
        // path is a *fallback*, not a merge target.
        //
        // Storage paths are derived from `tmp` so they're absolute on
        // every OS (Windows needs a drive-letter prefix to satisfy
        // `Path::is_absolute()`; a Unix-style `/a` is not absolute
        // there and gets joined to the config file's parent dir).
        let tmp = tempfile::tempdir().unwrap();
        let cli_storage = tmp.path().join("from-cli");
        let default_storage = tmp.path().join("from-default");
        let cli = write_yaml(
            tmp.path(),
            "explicit.yml",
            &format!("storage: {}\nuplinks: {{}}\npackages: {{}}\n", cli_storage.display()),
        );
        let default = write_yaml(
            tmp.path(),
            "default.yml",
            &format!("storage: {}\nuplinks: {{}}\npackages: {{}}\n", default_storage.display()),
        );
        let (config, source) = Config::resolve(Some(&cli), Some(&default), listen(), None).unwrap();
        assert_eq!(source, ConfigSource::Cli(cli));
        // Confirms the *content* came from the CLI file, not the default.
        assert_eq!(config.storage, cli_storage);
    }

    #[test]
    fn resolve_propagates_missing_file_error_for_cli_path() {
        let err = Config::resolve(Some(Path::new("/no/such/file.yml")), None, listen(), None)
            .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn resolve_propagates_parse_error_for_cli_path() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_yaml(tmp.path(), "broken.yml", "storage: [not, a, string\n");
        let err = Config::resolve(Some(&path), None, listen(), None).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn resolve_propagates_missing_file_error_for_default_path() {
        // Symmetric to the CLI case — a bad default path is just as
        // fatal as a bad CLI path. (In practice callers only pass a
        // default path that already passed `config_file_in`'s
        // `is_file()` check, so this is a defense-in-depth assertion.)
        let err = Config::resolve(None, Some(Path::new("/no/such/file.yml")), listen(), None)
            .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn resolve_public_url_override_threads_through() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_yaml(tmp.path(), "config.yaml", MINIMAL_YAML);
        let (config, _) =
            Config::resolve(Some(&path), None, listen(), Some("http://override.test".to_string()))
                .unwrap();
        assert_eq!(config.public_url, "http://override.test");
    }

    #[test]
    fn resolve_bundled_branch_honors_public_url_override() {
        let (config, source) =
            Config::resolve(None, None, listen(), Some("http://from-cli.test".to_string()))
                .unwrap();
        assert_eq!(source, ConfigSource::Bundled);
        assert_eq!(config.public_url, "http://from-cli.test");
    }

    // ----- serde defaults ---------------------------------------------------

    #[test]
    fn yaml_with_no_storage_uses_default_storage_string() {
        // `storage:` is absent entirely — `default_storage_string`
        // supplies `"./storage"`, which `resolve_relative` then joins
        // to the config-file's parent dir.
        let yaml = "uplinks: {}\npackages: {}\n";
        let config = Config::from_yaml_str(yaml, Path::new("/etc/pnpr"), listen(), None).unwrap();
        assert_eq!(config.storage, PathBuf::from("/etc/pnpr/./storage"));
    }

    #[test]
    fn yaml_log_block_with_no_type_field_uses_default_log_type() {
        // `type:` omitted but `format:` and `level:` present. The
        // `default_log_type` serde default kicks in for the missing
        // field; we don't otherwise care about its value at runtime,
        // we just need the parse to succeed (and the runtime config
        // to reflect the supplied format/level).
        let yaml = "\
storage: ./s
uplinks: {}
packages: {}
log:
  format: json
  level: warn
";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        assert_eq!(config.logs.format, LogFormat::Json);
        assert_eq!(config.logs.level, LogLevel::Warn);
        // `type:` omitted entirely falls back to the supported stdout sink.
        assert_eq!(config.logs.sink, "stdout");
        assert!(config.logs.sink_is_supported());
    }

    // ----- policy wiring from YAML ------------------------------------------

    #[test]
    fn policies_are_derived_from_packages_block() {
        // The `access` / `publish` tokens in each entry drive the
        // runtime policy — not a hard-coded default set.
        let yaml = "\
storage: ./s
uplinks: {}
packages:
  '@secret/*':
    access: $authenticated
    publish: $authenticated
  '**':
    access: $all
    publish: $authenticated
";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        let secret = config.policies.for_package("@secret/thing");
        assert!(!secret.access.allows(&Identity::Anonymous));
        assert!(secret.access.allows(&user("alice")));
        let public = config.policies.for_package("lodash");
        assert!(public.access.allows(&Identity::Anonymous));
        assert!(!public.publish.allows(&Identity::Anonymous));
    }

    #[test]
    fn policy_first_matching_rule_wins() {
        // `@secret/*` is declared before the `**` catch-all, so it
        // wins for a scoped package even though both match.
        let yaml = "\
storage: ./s
uplinks: {}
packages:
  '@secret/*':
    access: $authenticated
  '**':
    access: $all
";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        assert!(!config.policies.for_package("@secret/x").access.allows(&Identity::Anonymous));
        assert!(config.policies.for_package("anything").access.allows(&Identity::Anonymous));
    }

    #[test]
    fn policy_missing_access_and_publish_default_to_all_and_authenticated() {
        let yaml = "\
storage: ./s
uplinks: {}
packages:
  'lodash': {}
";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        let effective = config.policies.for_package("lodash");
        assert!(effective.access.allows(&Identity::Anonymous));
        assert!(!effective.publish.allows(&Identity::Anonymous));
        assert!(effective.publish.allows(&user("alice")));
    }

    #[test]
    fn policy_anonymous_token_is_wired() {
        let yaml = "\
storage: ./s
uplinks: {}
packages:
  '@anon/*':
    access: $anonymous
";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        let anon = config.policies.for_package("@anon/x");
        assert!(anon.access.allows(&Identity::Anonymous));
        assert!(!anon.access.allows(&user("alice")));
    }

    #[test]
    fn policy_usernames_grant_per_user_access() {
        // Bare names are usernames/groups (verdaccio-style), no longer
        // a config error.
        let yaml = "\
storage: ./s
uplinks: {}
packages:
  '@team/*':
    access: alice bob
    publish: alice
";
        let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
        let team = config.policies.for_package("@team/x");
        assert!(team.access.allows(&user("alice")));
        assert!(team.access.allows(&user("bob")));
        assert!(!team.access.allows(&user("carol")));
        assert!(!team.access.allows(&Identity::Anonymous));
        assert!(team.publish.allows(&user("alice")));
        assert!(!team.publish.allows(&user("bob")));
    }

    #[test]
    fn policy_access_list_accepts_string_and_sequence_forms() {
        // verdaccio accepts both a space-separated string and a YAML
        // sequence; they must compile to the same token list.
        let as_string = "\
storage: ./s
uplinks: {}
packages:
  '@team/*':
    access: alice bob
";
        let as_sequence = "\
storage: ./s
uplinks: {}
packages:
  '@team/*':
    access: [alice, bob]
";
        for yaml in [as_string, as_sequence] {
            let config = Config::from_yaml_str(yaml, Path::new("/x"), listen(), None).unwrap();
            let access = config.policies.for_package("@team/x").access;
            assert!(access.allows(&user("alice")), "{yaml}");
            assert!(access.allows(&user("bob")), "{yaml}");
            assert!(!access.allows(&user("carol")), "{yaml}");
        }
    }

    #[test]
    fn bundled_default_config_enforces_its_protections() {
        // Building from the bundled YAML must reproduce the
        // registry-mock protections that used to be hard-coded.
        let config = Config::from_default_yaml(Path::new("/tmp"), listen(), None);
        let needs_auth = config.policies.for_package("@pnpm.e2e/needs-auth");
        assert!(!needs_auth.access.allows(&Identity::Anonymous));
        assert!(needs_auth.access.allows(&user("alice")));
        assert!(!config.policies.for_package("@private/foo").access.allows(&Identity::Anonymous));
        let public = config.policies.for_package("lodash");
        assert!(public.access.allows(&Identity::Anonymous));
        assert!(!public.publish.allows(&Identity::Anonymous));
    }
}
