//! User and token storage for the registry.
//!
//! Auth state is split into two record stores, each behind a narrow
//! async trait so the backing store is config-selectable:
//!
//! * [`UserBackend`] — username → bcrypt-hashed password.
//! * [`TokenBackend`] — SHA-256 token hash → token record.
//!
//! Implementations are picked at startup by
//! [`AuthState::load`]:
//!
//! * [`UserStore`] / [`TokenStore`] — the local default. Users are an
//!   Apache-style htpasswd file; tokens a `SQLite` database. Each keeps
//!   a full mirror of its state in a `Mutex<...>` and persists on
//!   every write, so reads (the hot path for `enforce_access`) never
//!   touch disk. With no file configured both fall back to a pure
//!   in-memory map (the `@pnpm/registry-mock` shape).
//! * `backend.libsql`, `backend.postgres`, and `backend.mysql` —
//!   shared SQL databases that store both records in one place, so
//!   several stateless pnpr replicas observe a consistent set of users
//!   and tokens. The on-disk htpasswd format doesn't network, so users
//!   live in a `users` table here; the `tokens` table matches the local
//!   schema.
//!
//! The raw token is only ever returned to the caller once on `issue`;
//! only its SHA-256 hash hits storage, so a leak of the database
//! doesn't grant access on its own.

pub mod oidc;

pub use token_store::{TokenRecord, TokenStore};

mod htpasswd;
use htpasswd::{
    hash_bcrypt, parse_htpasswd, serialize_htpasswd, verify_returning_user, write_atomic,
};

mod token_store;
use token_store::{fresh_secret, sha256_hex};

use async_trait::async_trait;
#[cfg(feature = "backend-libsql")]
use libsql_backend::LibsqlAuth;
use pnpr_config::{AuthConfig, BackendConfig, MaxUsers};
use pnpr_error::{RegistryError, Result};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
#[cfg(feature = "backend-mysql")]
use sqlx_backend::mysql::MysqlAuth;
#[cfg(feature = "backend-postgres")]
use sqlx_backend::postgres::PostgresAuth;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(feature = "backend-libsql")]
mod libsql_backend;
#[cfg(any(feature = "backend-postgres", feature = "backend-mysql"))]
mod sqlx_backend;

/// Bound a read-only request-path or startup database future with a
/// deadline, surfacing [`RegistryError::AuthDatabaseTimeout`] on expiry.
/// Use only around reads and startup setup: request-path writes must
/// await the database result directly, so a caller never observes a
/// timeout with an unknown commit state.
#[cfg(any(feature = "backend-libsql", feature = "backend-postgres", feature = "backend-mysql"))]
async fn with_auth_timeout<Loaded, DbError>(
    deadline: std::time::Duration,
    future: impl std::future::Future<Output = std::result::Result<Loaded, DbError>>,
) -> Result<Loaded>
where
    RegistryError: From<DbError>,
{
    match tokio::time::timeout(deadline, future).await {
        Ok(result) => result.map_err(RegistryError::from),
        Err(_) => Err(RegistryError::AuthDatabaseTimeout),
    }
}

pub(crate) const MAX_USERNAME_CHARS: usize = 255;

pub(crate) fn validate_username(username: &str) -> Result<()> {
    if username.is_empty() {
        return Err(RegistryError::BadRequest { reason: "username must not be empty".to_string() });
    }
    if username.chars().count() > MAX_USERNAME_CHARS {
        return Err(RegistryError::BadRequest {
            reason: format!("username must be at most {MAX_USERNAME_CHARS} characters"),
        });
    }
    let reason = rejected_username_reason(username);
    match reason {
        Some(reason) => Err(RegistryError::BadRequest { reason: reason.to_string() }),
        None => Ok(()),
    }
}

/// Why a username of an acceptable length is still not one pnpr will store.
fn rejected_username_reason(username: &str) -> Option<&'static str> {
    let trimmed = username.trim_matches(char::is_whitespace);
    if trimmed.len() != username.len() {
        return Some("username must not start or end with whitespace");
    }
    if username.starts_with('#') {
        return Some("username must not start with '#'");
    }
    if username.contains(':') {
        return Some("username must not contain ':'");
    }
    if username.chars().any(char::is_control) {
        return Some("username must not contain control characters");
    }
    None
}

pub(crate) fn token_timestamp_from_sql(timestamp: i64) -> u64 {
    timestamp.max(0) as u64
}

pub(crate) fn token_timestamp_to_sql(timestamp: u64) -> i64 {
    i64::try_from(timestamp).unwrap_or(i64::MAX)
}

/// Bundle of the user store and the token store, each a trait object
/// so the rest of the server doesn't have to know whether auth is
/// file-backed, in-memory, or networked. Built once at startup by
/// [`Self::load`].
#[derive(Clone)]
pub struct AuthState {
    pub users: Arc<dyn UserBackend>,
    pub tokens: Arc<dyn TokenBackend>,
}

impl std::fmt::Debug for AuthState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("AuthState").finish_non_exhaustive()
    }
}

impl AuthState {
    /// All-in-memory auth state with open registration. Used by tests
    /// and registry-mock-compatible programmatic routers.
    #[must_use]
    pub fn in_memory() -> Self {
        Self::in_memory_with_max_users(MaxUsers::Unlimited)
    }

    /// All-in-memory auth state that enforces the resolved registration cap.
    #[must_use]
    pub fn in_memory_with_max_users(max_users: MaxUsers) -> Self {
        Self {
            users: Arc::new(UserStore::in_memory_with_max_users(max_users)),
            tokens: Arc::new(TokenStore::in_memory()),
        }
    }

    /// Build the auth state from the resolved config. A configured SQL
    /// backend backs both stores with one shared database; otherwise
    /// each local store is in-memory when its file path is unset and
    /// file-backed otherwise. The fallible step (open the htpasswd /
    /// `SQLite` file, or connect to the configured DB and ensure its
    /// schema) runs here so a malformed file or an unreachable database
    /// surfaces as a startup error before the socket is bound.
    pub async fn load(auth: &AuthConfig, backend: &BackendConfig) -> Result<Self> {
        match backend {
            BackendConfig::Local => {}
            BackendConfig::Libsql(settings) => return Self::load_libsql(auth, settings).await,
            BackendConfig::Postgres(settings) => return Self::load_postgres(auth, settings).await,
            BackendConfig::Mysql(settings) => return Self::load_mysql(auth, settings).await,
        }
        Self::load_local(auth)
    }

    async fn load_libsql(
        auth: &AuthConfig,
        settings: &pnpr_config::LibsqlSettings,
    ) -> Result<Self> {
        #[cfg(feature = "backend-libsql")]
        {
            let shared = Arc::new(LibsqlAuth::connect(settings, auth.htpasswd.max_users).await?);
            let users: Arc<dyn UserBackend> = Arc::clone(&shared) as Arc<dyn UserBackend>;
            let tokens: Arc<dyn TokenBackend> = shared;
            Ok(Self { users, tokens })
        }
        #[cfg(not(feature = "backend-libsql"))]
        {
            let _ = (auth, settings);
            Err(backend_not_enabled("libsql", "backend-libsql"))
        }
    }

    async fn load_postgres(
        auth: &AuthConfig,
        settings: &pnpr_config::SqlBackendSettings,
    ) -> Result<Self> {
        #[cfg(feature = "backend-postgres")]
        {
            let shared = Arc::new(PostgresAuth::connect(settings, auth.htpasswd.max_users).await?);
            let users: Arc<dyn UserBackend> = Arc::clone(&shared) as Arc<dyn UserBackend>;
            let tokens: Arc<dyn TokenBackend> = shared;
            Ok(Self { users, tokens })
        }
        #[cfg(not(feature = "backend-postgres"))]
        {
            let _ = (auth, settings);
            Err(backend_not_enabled("postgres", "backend-postgres"))
        }
    }

    async fn load_mysql(
        auth: &AuthConfig,
        settings: &pnpr_config::SqlBackendSettings,
    ) -> Result<Self> {
        #[cfg(feature = "backend-mysql")]
        {
            let shared = Arc::new(MysqlAuth::connect(settings, auth.htpasswd.max_users).await?);
            let users: Arc<dyn UserBackend> = Arc::clone(&shared) as Arc<dyn UserBackend>;
            let tokens: Arc<dyn TokenBackend> = shared;
            Ok(Self { users, tokens })
        }
        #[cfg(not(feature = "backend-mysql"))]
        {
            let _ = (auth, settings);
            Err(backend_not_enabled("mysql", "backend-mysql"))
        }
    }

    fn load_local(auth: &AuthConfig) -> Result<Self> {
        let users: Arc<dyn UserBackend> = match auth.htpasswd.file.clone() {
            Some(path) => Arc::new(UserStore::open(path, auth.htpasswd.max_users)?),
            None => Arc::new(UserStore::in_memory_with_max_users(auth.htpasswd.max_users)),
        };
        let tokens: Arc<dyn TokenBackend> = match auth.tokens.file.clone() {
            Some(path) => Arc::new(TokenStore::open(path)?),
            None => Arc::new(TokenStore::in_memory()),
        };
        Ok(Self { users, tokens })
    }
}

#[cfg(any(
    not(feature = "backend-libsql"),
    not(feature = "backend-postgres"),
    not(feature = "backend-mysql")
))]
fn backend_not_enabled(name: &str, feature: &str) -> RegistryError {
    RegistryError::InvalidConfig {
        reason: format!("backend.{name} is configured but pnpr was built without `{feature}`"),
    }
}

/// Username + password record store. The only operation is
/// [`Self::add_or_login`] (npm `adduser` / `login`), which verifies a
/// password and mints a bearer token. pnpr no longer verifies Basic
/// credentials on requests, so there is no per-request password check.
#[async_trait]
pub trait UserBackend: Send + Sync {
    /// Add a new user or verify a returning one. On success, returns
    /// the outcome plus the canonical stored username to bind follow-up
    /// token issuance to the same identity. A wrong password for an
    /// existing user is [`RegistryError::Unauthenticated`], and a new user
    /// past the registration cap is [`RegistryError::RegistrationDisabled`] /
    /// `TooManyUsers`.
    async fn add_or_login(&self, username: &str, password: &str)
    -> Result<(UpsertOutcome, String)>;
}

/// Bearer-token record store. The hot read is [`Self::lookup`]
/// (resolving the `Authorization: Bearer` header on nearly every
/// request); the rest back the `/-/npm/v1/tokens` CRUD endpoints.
#[async_trait]
pub trait TokenBackend: Send + Sync {
    /// Mint a fresh token for `username`, persist its hash, and return
    /// the raw token. The raw token is never stored.
    async fn issue(&self, username: &str) -> Result<String>;

    /// Resolve a raw token back to its username. `Ok(None)` means the
    /// token was never issued (or was revoked); `Err` means the backing
    /// store failed — never conflate the two, or a store outage reads as
    /// "not authenticated".
    async fn lookup(&self, raw: &str) -> Result<Option<String>>;

    /// Resolve a raw token to its full record — the username plus the
    /// `readonly` / `cidr_whitelist` restrictions that [`Self::lookup`]
    /// drops. The request-time restriction gate needs those, so it goes
    /// through here. `Ok(None)` for a token that was never issued (or was
    /// revoked); `Err` only for a backing-store failure, never conflated
    /// with "no such token".
    async fn lookup_record(&self, raw: &str) -> Result<Option<TokenRecord>> {
        self.find_by_key(&sha256_hex(raw.as_bytes())).await
    }

    /// Snapshot the record for a token by its key (SHA-256 hex). Used
    /// to check ownership before revocation. `Ok(None)` if no such
    /// token; `Err` on a store failure.
    async fn find_by_key(&self, key: &str) -> Result<Option<TokenRecord>>;

    /// All tokens owned by `username`, as `(key, record)` pairs where
    /// `key` is the SHA-256 hex digest the listing endpoint surfaces.
    async fn list_for_user(&self, username: &str) -> Result<Vec<(String, TokenRecord)>>;

    /// Remove a token by its key (the SHA-256 hex digest). Returns the
    /// deleted record so a higher layer can confirm the revocation.
    async fn revoke_by_key(&self, key: &str) -> Result<Option<TokenRecord>>;

    /// Remove a token by its raw value — the `DELETE /-/user/token/:tok`
    /// (npm logout) path puts the bearer token verbatim in the URL, so
    /// this hashes first and defers to [`Self::revoke_by_key`].
    async fn revoke_by_raw(&self, raw: &str) -> Result<Option<TokenRecord>> {
        self.revoke_by_key(&sha256_hex(raw.as_bytes())).await
    }
}

/// Bcrypt cost factor used for new password hashes. Cost 10 is what
/// verdaccio uses by default and matches Apache `htpasswd -B`'s
/// default, so files written here verify cleanly against either
/// tool. ~50–100 ms per hash on modern hardware — slow enough to
/// frustrate offline cracking, cheap enough that adduser doesn't
/// feel sluggish.
const DEFAULT_BCRYPT_COST: u32 = 10;

/// File-backed (or in-memory) htpasswd store.
#[derive(Debug)]
pub struct UserStore {
    /// `username -> bcrypt hash`. The hash string carries its own
    /// version and cost (`$2y$10$...`) so we never need to remember
    /// per-record metadata.
    users: Mutex<HashMap<String, String>>,
    path: Option<PathBuf>,
    max_users: MaxUsers,
    bcrypt_cost: u32,
}

impl UserStore {
    /// In-memory store with no on-disk persistence and open registration.
    /// Used by registry-mock-compatible programmatic routers.
    #[must_use]
    pub fn in_memory() -> Self {
        Self::in_memory_with_max_users(MaxUsers::Unlimited)
    }

    /// In-memory store that enforces the resolved registration cap.
    #[must_use]
    pub fn in_memory_with_max_users(max_users: MaxUsers) -> Self {
        Self {
            users: Mutex::new(HashMap::new()),
            path: None,
            max_users,
            bcrypt_cost: DEFAULT_BCRYPT_COST,
        }
    }

    /// File-backed store. The file is parsed up front so a malformed
    /// htpasswd surfaces as a startup error rather than a silent
    /// empty user list. A missing file is OK — it's created on the
    /// first registration.
    pub fn open(path: PathBuf, max_users: MaxUsers) -> Result<Self> {
        Self::open_with_cost(path, max_users, DEFAULT_BCRYPT_COST)
    }

    /// Like [`Self::open`] but with a configurable bcrypt cost — used
    /// by tests that want sub-100ms hashing.
    pub fn open_with_cost(path: PathBuf, max_users: MaxUsers, bcrypt_cost: u32) -> Result<Self> {
        let users = match std::fs::read_to_string(&path) {
            Ok(raw) => parse_htpasswd(&raw).map_err(|reason| {
                RegistryError::InvalidHtpasswdFile { path: path.display().to_string(), reason }
            })?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(err) => return Err(err.into()),
        };
        Ok(Self { users: Mutex::new(users), path: Some(path), max_users, bcrypt_cost })
    }

    /// Reject registration before spending time hashing a new password.
    fn check_registration_capacity(&self) -> Result<()> {
        match self.max_users {
            MaxUsers::Disabled => return Err(RegistryError::RegistrationDisabled),
            MaxUsers::Limited(max) => {
                let current = self.users.lock().expect("UserStore mutex poisoned").len() as u64;
                if current >= max {
                    return Err(RegistryError::TooManyUsers { max });
                }
            }
            MaxUsers::Unlimited => {}
        }
        Ok(())
    }

    async fn persist(&self, body: String) -> Result<()> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        tokio::task::spawn_blocking(move || write_atomic(&path, body.as_bytes())).await??;
        Ok(())
    }
}

#[async_trait]
impl UserBackend for UserStore {
    /// * Unknown username, registration allowed → bcrypt the password,
    ///   insert, persist, return `Created`.
    /// * Known username, password matches → return `LoggedIn`.
    /// * Known username, password wrong → `Unauthenticated`.
    /// * Unknown username, registration disabled or capped →
    ///   `RegistrationDisabled` / `TooManyUsers`.
    async fn add_or_login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(UpsertOutcome, String)> {
        validate_username(username)?;

        let existing_hash = {
            let users = self.users.lock().expect("UserStore mutex poisoned");
            users.get(username).cloned()
        };
        if let Some(stored) = existing_hash {
            return verify_returning_user(username, password, stored).await;
        }

        self.check_registration_capacity()?;

        let hash = hash_bcrypt(password.to_string(), self.bcrypt_cost).await?;
        enum NextStep {
            Persist(String),
            VerifyExisting(String),
        }
        let next_step = {
            let mut users = self.users.lock().expect("UserStore mutex poisoned");
            match (users.get(username).cloned(), self.max_users) {
                (Some(stored), _) => NextStep::VerifyExisting(stored),
                // Re-check under the lock because another registration may
                // have filled the store while we were hashing.
                (None, MaxUsers::Limited(max)) if users.len() as u64 >= max => {
                    return Err(RegistryError::TooManyUsers { max });
                }
                (None, _) => {
                    users.insert(username.to_string(), hash);
                    NextStep::Persist(serialize_htpasswd(&users))
                }
            }
        };
        match next_step {
            NextStep::Persist(snapshot) => {
                self.persist(snapshot).await?;
                Ok((UpsertOutcome::Created, username.to_string()))
            }
            NextStep::VerifyExisting(stored) => {
                verify_returning_user(username, password, stored).await
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum UpsertOutcome {
    /// The user didn't exist; we created the account.
    Created,
    /// The user existed and the password matched.
    LoggedIn,
}

impl Default for UserStore {
    fn default() -> Self {
        Self::in_memory()
    }
}

/// Identify the caller behind an HTTP request. Inspects the
/// `Authorization` header and resolves it to a username via the token
/// store. Only `Bearer` tokens are honored: pnpr does not accept Basic
/// credentials on requests. Clients authenticate with `_authToken`, and a
/// password login mints a token through [`UserBackend::add_or_login`] — so
/// request handling never pays a per-request bcrypt. Returns `None` for
/// missing/unsupported credentials so the caller can decide whether
/// anonymous is allowed.
///
/// The scheme is matched case-insensitively (RFC 7235 §2.1: "the
/// scheme is case-insensitive"), so `BEARER`, `bearer`, and `Bearer`
/// all parse the same.
/// `Ok(None)` covers every "no usable credentials" case — a missing or
/// malformed header, a non-`Bearer` scheme (including legacy `Basic`), or a
/// token that simply doesn't match. `Err` is reserved for a failure of the
/// backing store (e.g. the networked auth DB is unreachable) so the caller
/// can return a 5xx instead of a misleading 401.
pub async fn identify(
    header_value: Option<&str>,
    tokens: &dyn TokenBackend,
) -> Result<Option<String>> {
    let Some(value) = header_value.map(str::trim) else {
        return Ok(None);
    };
    let mut parts = value.splitn(2, ' ');
    let Some(scheme) = parts.next() else {
        return Ok(None);
    };
    let Some(credentials) = parts.next().map(str::trim) else {
        return Ok(None);
    };
    if scheme.eq_ignore_ascii_case("Bearer") {
        return tokens.lookup(credentials).await;
    }
    Ok(None)
}

/// `users` table DDL — only shared-database backends need it, since
/// the local backend keeps users in an htpasswd file. One bcrypt hash
/// per username, the same `$2y$...` string the htpasswd file would hold.
#[cfg(any(feature = "backend-libsql", feature = "backend-postgres", feature = "backend-mysql"))]
const USERS_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS users (
    username     VARCHAR(255) PRIMARY KEY,
    bcrypt_hash  TEXT NOT NULL
)";

#[cfg(any(feature = "backend-libsql", feature = "backend-postgres", feature = "backend-mysql"))]
const AUTH_COUNTERS_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS auth_counters (
    name   VARCHAR(64) PRIMARY KEY,
    value  BIGINT NOT NULL
)";

#[cfg(test)]
mod tests;
