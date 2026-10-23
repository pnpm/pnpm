use super::{
    AuthFile, BTreeSet, BackendFile, Deserialize, Duration, Identity, Interval, LibsqlFile, Path,
    PathBuf, RegistryError, ScimConfig, SqlBackendFile, build_admins,
    default_tokens_path_sibling_of, oidc, parse_interval, resolve_relative,
};

/// The resolved record-store backend for auth (users + tokens). Unlike
/// [`crate::HostedStoreConfig`], this only carries the parsed settings — the
/// fallible step (connecting to the database and ensuring its schema)
/// is async, so it runs in `AuthState::load` rather than at
/// config-parse time.
#[derive(Debug, Default, Clone)]
pub enum BackendConfig {
    /// Local htpasswd users + `SQLite` tokens (or in-memory when no file
    /// is configured).
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

/// The resolved record-store settings for libsql / Turso.
#[derive(Clone)]
pub struct LibsqlSettings {
    /// libsql/Turso database URL, e.g. `libsql://db.turso.io` or
    /// `http://127.0.0.1:8080` for a local `sqld`.
    pub url: String,
    /// Bearer token for the database. Omit for an unauthenticated local
    /// `sqld`.
    pub auth_token: Option<String>,
    /// Local path for an embedded replica. When set, the primary is
    /// replicated to this file and reads (the auth hot path) hit the
    /// local copy instead of a network round-trip per lookup; writes
    /// still go to the primary. Absent ⇒ every read is a remote query.
    pub replica_path: Option<PathBuf>,
    /// How often (seconds) the embedded replica pulls from the primary.
    /// Only meaningful with `replicaPath`; bounds how stale a read can
    /// be — most importantly, token-revocation lag. `0` disables
    /// background sync (the replica then reflects the initial sync at
    /// startup and its own account and token changes; a token's last-used
    /// time waits for the next sync). Defaults to
    /// [`LibsqlSettings::DEFAULT_SYNC_INTERVAL_SECS`].
    pub sync_interval_secs: Option<u64>,
    /// Most auth database operations running at once, each on its own
    /// connection. Defaults to [`LibsqlSettings::DEFAULT_MAX_CONNECTIONS`].
    pub max_connections: Option<u32>,
}

impl std::fmt::Debug for LibsqlSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LibsqlSettings")
            .field("url", &redact_connection_url(&self.url))
            .field("auth_token", &self.auth_token.as_ref().map(|_| "[redacted]"))
            .field("replica_path", &self.replica_path)
            .field("sync_interval_secs", &self.sync_interval_secs)
            .field("max_connections", &self.max_connections)
            .finish()
    }
}

impl LibsqlSettings {
    /// Default embedded-replica background sync cadence.
    pub const DEFAULT_SYNC_INTERVAL_SECS: u64 = 60;
    /// Default pool size, the one sqlx gives the other SQL backends.
    pub const DEFAULT_MAX_CONNECTIONS: u32 = 10;
}

/// The YAML `backend.postgres:` and `backend.mysql:` blocks. Whole-file
/// `${ENV}` substitution runs before parsing, so `url` can hold
/// `${...}` refs and keep credentials out of the committed config.
#[derive(Clone, Deserialize)]
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

impl std::fmt::Debug for SqlBackendSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlBackendSettings")
            .field("url", &redact_connection_url(&self.url))
            .field("max_connections", &self.max_connections)
            .field("timeout", &self.timeout)
            .field("startup_timeout", &self.startup_timeout)
            .finish()
    }
}

fn redact_connection_url(raw: &str) -> String {
    let Ok(mut parsed) = url::Url::parse(raw) else {
        return "[redacted]".to_string();
    };
    if parsed.password().is_some() {
        let _ = parsed.set_password(Some("[redacted]"));
    }
    if !parsed.username().is_empty() {
        let _ = parsed.set_username("[redacted]");
    }
    let query_pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(k, v)| {
            let k_lower = k.to_ascii_lowercase();
            if matches!(k_lower.as_str(), "password" | "token" | "authtoken" | "auth_token") {
                (k.into_owned(), "[redacted]".to_string())
            } else {
                (k.into_owned(), v.into_owned())
            }
        })
        .collect();
    if !query_pairs.is_empty() {
        parsed
            .query_pairs_mut()
            .clear()
            .extend_pairs(query_pairs);
    }
    parsed.to_string()
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
    /// The SCIM endpoints, off when `None`.
    pub scim: Option<ScimConfig>,
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
#[derive(Debug, Clone)]
pub struct IdentityConfig {
    /// Where to read/write the htpasswd-format user file and the
    /// token database. Both stores are in-memory when their paths
    /// are `None`.
    pub auth: AuthConfig,
    /// Which record store backs the auth state (users + tokens).
    /// Defaults to [`BackendConfig::Local`] — today's htpasswd file
    /// plus `SQLite` token database. The YAML `backend:` block can
    /// switch both stores to one shared SQL database so several
    /// stateless pnpr replicas see a consistent set of accounts.
    pub backend: BackendConfig,
    /// Usernames from `auth.admins`, who may administer every registry.
    pub admins: BTreeSet<String>,
}

impl IdentityConfig {
    #[must_use]
    pub fn is_admin(&self, identity: &Identity) -> bool {
        match identity {
            Identity::User { username, .. } => self.admins.contains(username),
            Identity::Anonymous | _ => false,
        }
    }

    /// Whether `username` is a configured admin, an OIDC login user,
    /// or an OIDC workload binding. Such names cannot be self-registered.
    #[must_use]
    pub fn is_reserved_name(&self, username: &str) -> bool {
        if self.admins.contains(username) {
            return true;
        }
        for provider in &self.auth.oidc {
            if provider.login
                .as_ref()
                .is_some_and(|login| {
                    login.users
                        .iter()
                        .any(|binding| binding.username == username)
                })
            {
                return true;
            }
            if provider.workloads
                .iter()
                .any(|workload| workload.identity.username == username)
            {
                return true;
            }
        }
        false
    }
}

pub(super) fn build_identity_config(
    file: &AuthFile,
    base_dir: &Path,
    backend: BackendConfig,
) -> Result<IdentityConfig, RegistryError> {
    if let Some(scim) = &file.scim {
        scim.validate()?;
    }
    Ok(IdentityConfig {
        admins: build_admins(&file.admins)?,
        auth: build_auth_config(file, base_dir),
        backend,
    })
}

pub(super) fn build_auth_config(file: &AuthFile, base_dir: &Path) -> AuthConfig {
    let htpasswd_file = file.htpasswd.file.as_deref().map(|raw| resolve_relative(raw, base_dir));
    let tokens_file = file.tokens.file
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
        scim: file.scim.clone(),
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
    if let Some(settings) = file.libsql {
        let libsql_settings = build_libsql_settings(settings, base_dir)?;
        selected.push(("libsql", BackendConfig::Libsql(libsql_settings)));
    }
    if let Some(settings) = file.postgres {
        selected.push((
            "postgres",
            BackendConfig::Postgres(build_sql_backend_settings("postgres", settings)?),
        ));
    }
    if let Some(settings) = file.mysql {
        selected.push((
            "mysql",
            BackendConfig::Mysql(build_sql_backend_settings("mysql", settings)?),
        ));
    }
    select_backend(selected)
}

fn select_backend(
    mut selected: Vec<(&str, BackendConfig)>,
) -> Result<BackendConfig, RegistryError> {
    match selected.len() {
        0 => Err(RegistryError::InvalidConfig {
            reason: "backend must select exactly one database backend".to_string(),
        }),
        1 => Ok(selected.remove(0).1),
        _ => {
            let names = selected
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>()
                .join(", ");
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
        parse_interval(value)
            .ok_or_else(|| RegistryError::InvalidConfig {
                reason: format!("backend.{backend}.{field} has an invalid interval {value:?}"),
            })
    })
    .transpose()
}

pub(super) fn build_libsql_settings(
    file: LibsqlFile,
    base_dir: &Path,
) -> Result<LibsqlSettings, RegistryError> {
    let replica_path =
        file.replica_path.map(|path| if path.is_absolute() { path } else { base_dir.join(path) });
    let sync_interval_secs = match &file.sync_interval {
        Some(interval) => {
            let dur = parse_interval(&interval.0)
                .ok_or_else(|| RegistryError::InvalidConfig {
                    reason: format!(
                        "backend.libsql.syncInterval {:?} is not a valid interval",
                        interval.0,
                    ),
                })?;
            if !dur.is_zero() && dur.as_secs() == 0 {
                return Err(RegistryError::InvalidConfig {
                    reason: format!(
                        "backend.libsql.syncInterval {:?} must be 0 or at least 1s",
                        interval.0,
                    ),
                });
            }
            Some(dur.as_secs())
        }
        None => None,
    };
    if file.max_connections == Some(0) {
        return Err(RegistryError::InvalidConfig {
            reason: "backend.libsql.maxConnections must be greater than 0".to_string(),
        });
    }
    Ok(LibsqlSettings {
        url: file.url,
        auth_token: file.auth_token,
        replica_path,
        sync_interval_secs,
        max_connections: file.max_connections,
    })
}
