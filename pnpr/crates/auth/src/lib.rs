#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

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
pub mod schema;
pub(crate) mod validation;

pub use htpasswd::{
    is_bcrypt_hash, is_sha_hash, parse_htpasswd, parse_htpasswd_line, verify_bcrypt, write_atomic,
};
pub use identify::identify;
pub use last_used::{LAST_USED_GRANULARITY_SECS, LastUsedWriter};
pub use owned_tokens::OwnedTokens;
pub use schema::CURRENT_SCHEMA_VERSION;
pub use token_store::{NewToken, TokenRecord, TokenStore};
pub use user_store::UserStore;
pub use validation::MAX_PASSWORD_BYTES;

mod htpasswd;
use htpasswd::{dummy_verify, hash_bcrypt, serialize_htpasswd, verify_returning_user};

mod token_store;
use token_store::{fresh_secret, sha256_hex, unix_seconds};

mod identify;
mod last_used;
mod owned_tokens;
mod user_store;

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
    collections::{HashMap, HashSet},
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

/// Bundle of the user store and the token store, each a trait object
/// so the rest of the server doesn't have to know whether auth is
/// file-backed, in-memory, or networked. Built once at startup by
/// [`Self::load`].
#[derive(Clone)]
pub struct AuthState {
    pub users: Arc<dyn UserBackend>,
    pub tokens: Arc<dyn TokenBackend>,
}

/// The outcome of [`AuthState::put_user`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserPut {
    Created,
    PasswordChanged,
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
        let users = Arc::new(UserStore::in_memory_with_max_users(max_users));
        let tokens = OwnedTokens::new(TokenStore::in_memory(), Arc::clone(&users));
        Self { users, tokens: Arc::new(tokens) }
    }

    /// Revoke every token `username` holds. Returns how many were revoked.
    pub async fn revoke_all_tokens(&self, username: &str) -> Result<usize> {
        self.tokens.revoke_all_for_user(username).await
    }

    /// Create `username` with `password`, or change the password of the
    /// account that exists. Tokens a removed account left under the name are
    /// revoked first, so they never reach the new account. `None` means
    /// another writer removed the account between the two steps.
    pub async fn put_user(&self, username: &str, password: &str) -> Result<Option<UserPut>> {
        if self.users.password_hash(username).await?.is_none() {
            self.revoke_all_tokens(username).await?;
        }
        match self.users.create_user(username, password).await? {
            UserCreation::Created => Ok(Some(UserPut::Created)),
            UserCreation::NameTaken => match self.users.set_password(username, password).await? {
                PasswordChange::Changed => Ok(Some(UserPut::PasswordChanged)),
                // Another caller removed the user between the two calls.
                PasswordChange::NoSuchUser => Ok(None),
            },
        }
    }

    /// Check that the auth backends (users and tokens) are reachable and ready.
    pub async fn check_readiness(&self) -> Result<()> {
        self.users.check_readiness().await?;
        self.tokens.check_readiness().await?;
        Ok(())
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
            BackendConfig::Libsql(settings) => {
                return Self::load_libsql(auth, settings).await;
            }
            BackendConfig::Postgres(settings) => {
                return Self::load_postgres(auth, settings).await;
            }
            BackendConfig::Mysql(settings) => {
                return Self::load_mysql(auth, settings).await;
            }
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
        let users = Arc::new(match auth.htpasswd.file.clone() {
            Some(path) => UserStore::open(path, auth.htpasswd.max_users)?,
            None => UserStore::in_memory_with_max_users(auth.htpasswd.max_users),
        });
        let tokens = match auth.tokens.file.clone() {
            Some(path) => TokenStore::open(path)?,
            None => TokenStore::in_memory(),
        };
        let tokens = Arc::new(OwnedTokens::new(tokens, Arc::clone(&users)));
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

/// Username + password record store. Clients reach it through
/// [`Self::add_or_login`] (npm `adduser` / `login`); the other operations
/// back the admin API. pnpr does not verify Basic credentials on requests,
/// so there is no per-request password check.
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

    /// Every stored username, sorted.
    async fn list_users(&self) -> Result<Vec<String>>;

    /// The stored bcrypt hash of `username`, or `None` when there is no
    /// such user. The hash is salted, so it differs for every account and
    /// every password set: equal hashes mean the same account with the same
    /// password.
    async fn password_hash(&self, username: &str) -> Result<Option<String>>;

    /// Create `username` on an administrator's behalf. Not bound by the
    /// self-registration cap.
    async fn create_user(&self, username: &str, password: &str) -> Result<UserCreation>;

    /// Replace the password of `username`.
    async fn set_password(&self, username: &str, password: &str) -> Result<PasswordChange>;

    /// Remove `username`. The user's tokens are not touched.
    async fn delete_user(&self, username: &str) -> Result<UserRemoval>;

    /// Check that the user store backend is reachable and ready to process requests.
    async fn check_readiness(&self) -> Result<()> {
        Ok(())
    }
}

/// Bearer-token record store. The hot read is [`Self::lookup`]
/// (resolving the `Authorization: Bearer` header on nearly every
/// request); the rest back the `/-/npm/v1/tokens` CRUD endpoints.
#[async_trait]
pub trait TokenBackend: Send + Sync {
    /// Check that the token store backend is reachable and ready to process requests.
    async fn check_readiness(&self) -> Result<()> {
        Ok(())
    }

    /// Mint a fresh token for `username` with custom settings, persist its hash,
    /// and return the raw token and the stored record.
    async fn issue_token(
        &self,
        username: &str,
        new_token: NewToken,
    ) -> Result<(String, TokenRecord)>;

    /// Mint a default read-write token for `username`, persist its hash, and return
    /// the raw token. The raw token is never stored.
    async fn issue(&self, username: &str) -> Result<String> {
        let (raw, _) = self.issue_token(username, NewToken::default()).await?;
        Ok(raw)
    }

    /// Resolve a raw token back to its username. `Ok(None)` means the
    /// token was never issued, was revoked, or expired; `Err` means the backing
    /// store failed — never conflate the two, or a store outage reads as
    /// "not authenticated".
    async fn lookup(&self, raw: &str) -> Result<Option<String>>;

    /// Resolve a raw token to its full record — the username plus the
    /// `readonly`, `cidr_whitelist`, and `expires_at` restrictions that
    /// [`Self::lookup`] drops. Returns `Ok(None)` for a token that was never
    /// issued, was revoked, or expired.
    async fn lookup_record(&self, raw: &str) -> Result<Option<TokenRecord>> {
        let now = unix_seconds();
        let record = self.find_by_key(&sha256_hex(raw.as_bytes())).await?;
        if let Some(ref r) = record
            && r.expires_at.is_some_and(|exp| exp <= now)
        {
            return Ok(None);
        }
        Ok(record)
    }

    /// Update the last-used timestamp of the token identified by `key`.
    async fn touch_key(&self, _key: &str, _now: u64) -> Result<()> {
        Ok(())
    }

    /// Update the last-used timestamp of the token by raw bearer token value.
    async fn touch(&self, raw: &str, now: u64) -> Result<()> {
        self.touch_key(&sha256_hex(raw.as_bytes()), now).await
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

    /// Remove every token `username` holds. Returns how many were removed.
    async fn revoke_all_for_user(&self, username: &str) -> Result<usize> {
        let mut revoked = 0;
        for (key, _) in &self.list_for_user(username).await? {
            if self.revoke_by_key(key).await?.is_some() {
                revoked += 1;
            }
        }
        Ok(revoked)
    }
}

/// Bcrypt cost factor used for new password hashes. Cost 10 is what
/// verdaccio uses by default and matches Apache `htpasswd -B`'s
/// default, so files written here verify cleanly against either
/// tool. ~50–100 ms per hash on modern hardware — slow enough to
/// frustrate offline cracking, cheap enough that adduser doesn't
/// feel sluggish.
const DEFAULT_BCRYPT_COST: u32 = 10;

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertOutcome {
    /// The user didn't exist; we created the account.
    Created,
    /// The user existed and the password matched.
    LoggedIn,
}

/// What [`UserBackend::create_user`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserCreation {
    /// The name was free; the account is stored.
    Created,
    /// The name was taken, so the account it names is untouched.
    NameTaken,
}

/// What [`UserBackend::set_password`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordChange {
    /// The account's password is the new one.
    Changed,
    /// There is no such account, so no password was stored.
    NoSuchUser,
}

/// What [`UserBackend::delete_user`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserRemoval {
    /// The account is gone.
    Removed,
    /// There was no such account.
    NoSuchUser,
}

/// `users` table DDL — only shared-database backends need it, since
/// the local backend keeps users in an htpasswd file. One bcrypt hash
/// per username, the same `$2y$...` string the htpasswd file would hold.
#[cfg(any(feature = "backend-libsql", feature = "backend-postgres", feature = "backend-mysql"))]
pub(crate) const USERS_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS users (
    username     VARCHAR(255) PRIMARY KEY,
    bcrypt_hash  TEXT NOT NULL
)";

#[cfg(any(feature = "backend-libsql", feature = "backend-postgres", feature = "backend-mysql"))]
pub(crate) const AUTH_COUNTERS_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS auth_counters (
    name   VARCHAR(64) PRIMARY KEY,
    value  BIGINT NOT NULL
)";

#[cfg(test)]
mod tests;
