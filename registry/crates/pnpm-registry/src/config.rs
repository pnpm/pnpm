use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use indexmap::IndexMap;
use pacquet_env_replace::{SystemEnv, env_replace_lossy};
use serde::Deserialize;

use crate::policy::PackagePolicies;

/// The bundled verdaccio-shaped YAML config, mirrored from
/// `@pnpm/registry-mock`'s `registry/config.yaml`. Other crates can
/// pull this in directly when they need pnpm-registry's defaults
/// (uplinks, package routing) without reading a file from disk —
/// e.g. test mocks that want to run with the standard `**` -> `npmjs`
/// routing applied.
pub const DEFAULT_CONFIG_YAML: &str = include_str!("../config.yaml");

/// Runtime configuration for the pnpm registry server.
///
/// The persisted (YAML) shape follows verdaccio's `config.yaml` —
/// `storage`, `uplinks`, `packages` — restricted to the subset
/// pnpm-registry implements (no web UI, auth, plugins, or logs
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
    /// Per-package access and publish rules. Defaults to
    /// [`PackagePolicies::registry_mock_defaults`] so a vanilla
    /// `Config::proxy` / `Config::static_serve` enforces the same
    /// `@private/*` and `@pnpm.e2e/needs-auth` policies that
    /// `@pnpm/registry-mock` did under verdaccio.
    pub policies: PackagePolicies,
    /// Where to read/write the htpasswd-format user file and the
    /// token database. Both stores are in-memory when their paths
    /// are `None`, matching the original `@pnpm/registry-mock` mode
    /// where every restart wipes accounts.
    pub auth: AuthConfig,
    /// Format and level for the `tracing-subscriber` the binary
    /// installs at startup. Sourced from the first entry of the
    /// YAML `logs:` list. Defaults to pretty/info.
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

/// Runtime logging configuration. Mirrors the first entry of the
/// YAML `logs:` list (verdaccio's shape). Drives the
/// `tracing-subscriber` init in the binary: format selects
/// human-readable vs NDJSON, level seeds the default `EnvFilter`.
///
/// Only `type: stdout` is supported — file sinks and multiple-sink
/// fan-out are future work. The full list is parsed from YAML so
/// nothing breaks when a verdaccio user copies their config in;
/// extra entries beyond the first are dropped.
#[derive(Debug, Clone)]
pub struct LogConfig {
    pub format: LogFormat,
    pub level: LogLevel,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self { format: LogFormat::Pretty, level: LogLevel::default() }
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
    /// plus a `pnpm_registry::access=info` target so the per-request
    /// access log surfaces even when the rest of the crate is
    /// quieter.
    pub fn as_filter_directive(self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Http => "info,pnpm_registry::access=info",
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

/// Per-package routing rules. `access` and `publish` are parsed for
/// config compatibility but ignored (pnpm-registry is read-only and
/// has no auth). `proxy` selects the [`UplinkConfig`] by name.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct PackageAccess {
    pub access: Option<String>,
    pub publish: Option<String>,
    pub unpublish: Option<String>,
    pub proxy: Option<String>,
}

/// Disk shape of the YAML file. Fields verdaccio supports but
/// pnpm-registry doesn't (`auth`, `web`, `plugins`, `middlewares`,
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
    #[allow(dead_code)] // file/syslog sinks are future work
    r#type: String,
    #[serde(default)]
    format: Option<LogFormat>,
    #[serde(default)]
    level: Option<LogLevel>,
}

fn default_log_type() -> String {
    "stdout".to_string()
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
        let raw = std::fs::read_to_string(path)?;
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

    /// Resolve the auto-discovery path for a config file. Returns
    /// `Some(<home>/.config/pnpm-registry/config.yaml)` when both
    /// `home` is provided **and** that file exists; returns `None`
    /// otherwise. Callers (typically the binary's `main`) then fall
    /// back to [`Self::from_default_yaml`] for the bundled config.
    ///
    /// The function takes `home` as a parameter rather than calling
    /// `home::home_dir()` directly so tests can drive it with a
    /// `TempDir` without racing on the global `$HOME` env var.
    pub fn auto_config_path(home: Option<&Path>) -> Option<PathBuf> {
        let home = home?;
        let path = home.join(".config").join("pnpm-registry").join("config.yaml");
        path.is_file().then_some(path)
    }

    fn from_yaml_str(
        raw: &str,
        base_dir: &Path,
        listen: SocketAddr,
        public_url: Option<String>,
    ) -> Result<Self, serde_saphyr::Error> {
        let (substituted, unresolved) = env_replace_lossy::<SystemEnv>(raw);
        if !unresolved.is_empty() {
            tracing::warn!(?unresolved, "config references unset environment variables");
        }
        let file: ConfigFile = serde_saphyr::from_str(&substituted)?;
        let storage = resolve_relative(&file.storage, base_dir);
        let public_url = public_url.unwrap_or_else(|| format!("http://{listen}"));
        let auth = build_auth_config(&file.auth, base_dir);
        let logs = build_log_config(file.log.as_ref());
        Ok(Self {
            listen,
            public_url,
            storage,
            uplinks: file.uplinks,
            packages: file.packages,
            packument_ttl: Self::DEFAULT_PACKUMENT_TTL,
            // Policies could be derived from `packages[*].{access,publish}`
            // here, but the bundled `config.yaml` already matches the
            // `registry_mock_defaults` set verbatim, and a YAML-driven
            // policy wiring is out of scope for this rebase. Keep the
            // hard-coded defaults — same as `proxy` / `static_serve`.
            policies: PackagePolicies::registry_mock_defaults(),
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
    LogConfig { format: entry.format.unwrap_or_default(), level: entry.level.unwrap_or_default() }
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
        Config, DEFAULT_CONFIG_YAML, LogFormat, LogLevel, pattern_matches, resolve_relative,
    };
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::path::{Path, PathBuf};

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

    // ----- auto_config_path -------------------------------------------------

    #[test]
    fn auto_config_path_returns_none_when_home_is_none() {
        assert!(Config::auto_config_path(None).is_none());
    }

    #[test]
    fn auto_config_path_returns_none_when_file_is_missing() {
        // A fresh tempdir has no `.config/pnpm-registry/config.yaml`.
        let home = tempfile::tempdir().unwrap();
        assert!(Config::auto_config_path(Some(home.path())).is_none());
    }

    #[test]
    fn auto_config_path_returns_path_when_file_exists() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".config").join("pnpm-registry");
        std::fs::create_dir_all(&dir).unwrap();
        let expected = dir.join("config.yaml");
        std::fs::write(&expected, "storage: ./s\nuplinks: {}\npackages: {}\n").unwrap();
        let resolved = Config::auto_config_path(Some(home.path())).expect("file is present");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn auto_config_path_rejects_a_directory_at_the_target() {
        // If the path exists but is a directory (or symlink to one,
        // etc.), `is_file()` returns false. Auto-discovery should
        // bail rather than try to read it.
        let home = tempfile::tempdir().unwrap();
        let target = home.path().join(".config").join("pnpm-registry").join("config.yaml");
        std::fs::create_dir_all(&target).unwrap();
        assert!(Config::auto_config_path(Some(home.path())).is_none());
    }

    #[test]
    fn auto_config_path_resolved_file_round_trips_through_from_yaml() {
        // The whole point of returning a path is that `from_yaml` can
        // load it. This is the end-to-end happy path for the
        // auto-discovery flow.
        //
        // The `storage:` value is computed at runtime so it's a
        // genuinely absolute path on whichever OS the test runs on
        // (Windows requires a drive-letter prefix to satisfy
        // `Path::is_absolute()`; a Unix-style "/tmp/auto" is not
        // absolute there and gets joined to the config's parent dir).
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".config").join("pnpm-registry");
        std::fs::create_dir_all(&dir).unwrap();
        let storage = home.path().join("registry-storage");
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
        std::fs::write(dir.join("config.yaml"), yaml).unwrap();
        let path = Config::auto_config_path(Some(home.path())).unwrap();
        let config = Config::from_yaml(&path, listen(), None).unwrap();
        assert_eq!(config.storage, storage);
        assert_eq!(config.logs.format, LogFormat::Json);
        assert_eq!(config.logs.level, LogLevel::Info);
        assert_eq!(config.resolve_uplink("lodash").unwrap().0, "npmjs");
    }

    // ----- LogFormat / LogLevel serde behavior ------------------------------

    /// Helper: deserialize a YAML scalar into the requested enum.
    /// Lets us assert the variant mapping concisely.
    fn parse_log_yaml<T: serde::de::DeserializeOwned>(yaml: &str) -> Result<T, String> {
        serde_saphyr::from_str::<T>(yaml).map_err(|err| err.to_string())
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
}
