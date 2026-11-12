use super::{
    AuthFile, BTreeSet, Deserialize, Duration, Identity, Interval, LibsqlFile, Path, PathBuf,
    RegistryError, ScimConfig, SqlBackendFile, build_admins, default_tokens_path_sibling_of, oidc,
    parse_interval, resolve_relative,
};
use pnpr_error::redact_connection_url;

/// The resolved record-store backend for auth (users + tokens). Unlike
/// [`crate::HostedStoreConfig`], this only carries the parsed settings — the
/// fallible step (connecting to the database and ensuring its schema)
/// is async, so it runs in `AuthState::load` rather than at
/// config-parse time.
#[derive(Debug, Default, Clone)]
pub enum BackendConfig {
    /// Local JSON accounts + `SQLite` tokens (or in-memory when no file
    /// is configured).
    #[default]
    Local,
    /// Networked `SQLite` (libsql / Turso): both records live in one
    /// shared database reachable over the network.
    Libsql(LibsqlSettings),
    /// `PostgreSQL`: both records live in one shared database.
    Postgres(SqlBackendSettings),
    /// `MySQL` database: both records live in one shared database.
    Mysql(SqlBackendSettings),
}

/// The resolved settings of a libsql / Turso database.
#[derive(Clone, PartialEq, Eq)]
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

/// The settings of a `PostgreSQL` or `MySQL` `database:` block. Whole-file
/// `${ENV}` substitution runs before parsing, so `url` can hold
/// `${...}` refs and keep credentials out of the committed config.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlBackendSettings {
    /// Driver connection URL, e.g. `postgres://user:pass@host/db` or
    /// `mysql://user:pass@host/db`.
    pub url: String,
    /// Maximum connections in the backend pool. Defaults to the
    /// driver's pool default when omitted.
    pub max_connections: Option<u32>,
    /// Deadline for database operations.
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
    pub users: UsersConfig,
    pub tokens: TokensConfig,
    /// The SCIM endpoints, off when `None`.
    pub scim: Option<ScimConfig>,
}

/// Where the JSON account file lives and how many users may sign
/// up before registration is refused.
#[derive(Debug, Default, Clone)]
pub struct UsersConfig {
    /// Absolute path to the account file. `None` keeps user state
    /// in memory.
    pub file: Option<PathBuf>,
    /// Cap on new user registrations.
    pub max_users: MaxUsers,
}

/// Where the token database lives. SQLite-backed when `file` is
/// set; an in-memory map otherwise.
#[derive(Debug, Default, Clone)]
pub struct TokensConfig {
    pub file: Option<PathBuf>,
    /// The longest a bearer token issued by a login or `POST /-/npm/v1/tokens`
    /// stays valid. `None` lets a token never expire.
    pub max_lifetime: Option<Duration>,
}

/// Three-state cap on `auth.users.max_users`:
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
/// `./users.json` lives next to the config file (verdaccio's
/// convention). When `auth.users.file` is set but
/// `auth.tokens.file` is not, tokens default to a `tokens.db`
/// sibling of the account file — keeping credentials co-located in
/// one directory the operator can lock down (`chmod 600`).
#[derive(Debug, Clone)]
pub struct IdentityConfig {
    /// Where to read/write the JSON account file and the
    /// token database. Both stores are in-memory when their paths
    /// are `None`.
    pub auth: AuthConfig,
    /// Which record store backs the auth state (users + tokens).
    /// Defaults to [`BackendConfig::Local`] — a JSON account file
    /// plus `SQLite` token database. The YAML `database:` block can
    /// switch both stores to one shared SQL database so several
    /// stateless pnpr replicas see a consistent set of accounts.
    pub backend: BackendConfig,
    /// Account IDs from `auth.admins`, who may administer every registry.
    pub admins: BTreeSet<String>,
}

impl IdentityConfig {
    #[must_use]
    pub fn is_admin(&self, identity: &Identity) -> bool {
        match identity {
            Identity::User { id, .. } => self.admins.contains(id),
            Identity::Anonymous | _ => false,
        }
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
        auth: build_auth_config(file, base_dir)?,
        backend,
    })
}

pub(super) fn build_auth_config(
    file: &AuthFile,
    base_dir: &Path,
) -> Result<AuthConfig, RegistryError> {
    let users_file = file.users.file.as_deref().map(|raw| resolve_relative(raw, base_dir));
    let tokens_file = file.tokens.file
        .as_deref()
        .map(|raw| resolve_relative(raw, base_dir))
        .or_else(|| users_file.as_deref().map(default_tokens_path_sibling_of));
    Ok(AuthConfig {
        oidc: file.oidc.clone(),
        users: UsersConfig {
            file: users_file,
            max_users: file.users.max_users.map_or(MaxUsers::Disabled, MaxUsers::from_yaml),
        },
        tokens: TokensConfig {
            file: tokens_file,
            max_lifetime: build_max_token_lifetime(file.tokens.max_lifetime.as_ref())?,
        },
        scim: file.scim.clone(),
    })
}

/// The `auth.tokens.maxLifetime` key. Zero is refused: no token could be
/// issued.
fn build_max_token_lifetime(raw: Option<&Interval>) -> Result<Option<Duration>, RegistryError> {
    let Some(Interval(raw)) = raw else {
        return Ok(None);
    };
    match parse_interval(raw) {
        Some(lifetime) if !lifetime.is_zero() => Ok(Some(lifetime)),
        _ => Err(RegistryError::InvalidConfig {
            reason: format!("auth.tokens.maxLifetime {raw:?} is not a positive interval"),
        }),
    }
}

pub(super) fn build_sql_backend_settings(
    file: SqlBackendFile,
) -> Result<SqlBackendSettings, RegistryError> {
    let timeout = parse_backend_interval("timeout", file.timeout.as_ref())?
        .unwrap_or(SqlBackendSettings::DEFAULT_TIMEOUT);
    if timeout.is_zero() {
        return Err(RegistryError::InvalidConfig {
            reason: "database.timeout must be greater than 0".to_string(),
        });
    }
    let startup_timeout = parse_backend_interval("startupTimeout", file.startup_timeout.as_ref())?
        .unwrap_or(SqlBackendSettings::DEFAULT_STARTUP_TIMEOUT);
    if startup_timeout.is_zero() {
        return Err(RegistryError::InvalidConfig {
            reason: "database.startupTimeout must be greater than 0".to_string(),
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
    field: &str,
    raw: Option<&Interval>,
) -> Result<Option<Duration>, RegistryError> {
    raw.map(|Interval(value)| {
        parse_interval(value)
            .ok_or_else(|| RegistryError::InvalidConfig {
                reason: format!("database.{field} has an invalid interval {value:?}"),
            })
    })
    .transpose()
}

/// The settings `file` gives, `key` naming its block in errors.
pub(crate) fn build_libsql_settings(
    file: LibsqlFile,
    base_dir: &Path,
    key: &str,
) -> Result<LibsqlSettings, RegistryError> {
    let replica_path =
        file.replica_path.map(|path| if path.is_absolute() { path } else { base_dir.join(path) });
    let sync_interval_secs = match &file.sync_interval {
        Some(interval) => {
            let dur = parse_interval(&interval.0)
                .ok_or_else(|| RegistryError::InvalidConfig {
                    reason: format!("{key}.syncInterval {:?} is not a valid interval", interval.0),
                })?;
            if !dur.is_zero() && dur.as_secs() == 0 {
                return Err(RegistryError::InvalidConfig {
                    reason: format!("{key}.syncInterval {:?} must be 0 or at least 1s", interval.0),
                });
            }
            Some(dur.as_secs())
        }
        None => None,
    };
    if file.max_connections == Some(0) {
        return Err(RegistryError::InvalidConfig {
            reason: format!("{key}.maxConnections must be greater than 0"),
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

pub(super) fn build_backend_config(metadata: &crate::MetadataBackend) -> BackendConfig {
    use crate::MetadataBackend;
    match metadata {
        MetadataBackend::Sqlite(_) => BackendConfig::Local,
        MetadataBackend::Libsql(settings) => {
            let mut auth = settings.clone();
            // Independent sync schedules need independent replica files.
            auth.replica_path = auth.replica_path.map(|path| {
                let mut path = path.into_os_string();
                path.push(".auth");
                PathBuf::from(path)
            });
            BackendConfig::Libsql(auth)
        }
        MetadataBackend::Postgres(settings) => BackendConfig::Postgres(settings.clone()),
        MetadataBackend::Mysql(settings) => BackendConfig::Mysql(settings.clone()),
    }
}
