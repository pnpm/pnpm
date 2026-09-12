use super::{
    AuthFile, BackendFile, Deserialize, Duration, Interval, Path, PathBuf, RegistryError,
    SqlBackendFile, default_tokens_path_sibling_of, oidc, parse_interval, resolve_relative,
};

/// The resolved record-store backend for auth (users + tokens). Unlike
/// [`crate::HostedStoreConfig`], this only carries the parsed settings — the
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
    pub oidc: Vec<oidc::OidcProvider>,
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
    pub(super) fn from_yaml(value: i64) -> Self {
        if value < 0 { MaxUsers::Disabled } else { MaxUsers::Limited(value as u64) }
    }
}

/// Build the runtime [`AuthConfig`] from the YAML `auth:` block.
/// Relative paths are resolved against `base_dir` so a path like
/// `./htpasswd` lives next to the config file (verdaccio's
/// convention). When `auth.htpasswd.file` is set but
/// `auth.tokens.file` is not, tokens default to a `tokens.db`
/// sibling of the htpasswd file — keeping credentials co-located in
/// one directory the operator can lock down (`chmod 600`).
pub(super) fn build_auth_config(file: &AuthFile, base_dir: &Path) -> AuthConfig {
    let htpasswd_file = file.htpasswd.file.as_deref().map(|raw| resolve_relative(raw, base_dir));
    let tokens_file = file
        .tokens
        .file
        .as_deref()
        .map(|raw| resolve_relative(raw, base_dir))
        .or_else(|| htpasswd_file.as_deref().map(default_tokens_path_sibling_of));
    AuthConfig {
        oidc: file.oidc.clone(),
        htpasswd: HtpasswdConfig {
            file: htpasswd_file,
            max_users: file.htpasswd.max_users.map_or(MaxUsers::Disabled, MaxUsers::from_yaml),
        },
        tokens: TokensConfig { file: tokens_file },
    }
}

pub(super) fn build_backend_config(
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

pub(super) fn build_sql_backend_settings(
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

pub(super) fn parse_backend_interval(
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

pub(super) fn resolve_libsql_paths(settings: &mut LibsqlSettings, base_dir: &Path) {
    // Resolve a relative `replicaPath` against the config file's
    // directory, the same convention `storage` and the auth files
    // follow, so `./auth-replica.db` lands next to the config rather
    // than in the process CWD.
    if let Some(path) = settings.replica_path.take() {
        settings.replica_path = Some(if path.is_absolute() { path } else { base_dir.join(path) });
    }
}
