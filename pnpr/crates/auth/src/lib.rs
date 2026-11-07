#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

//! Account identity, password credentials, and bearer tokens for the registry.
//!
//! Accounts have immutable IDs and mutable login names. Tokens reference IDs.
//! [`UserStore`] persists native JSON account records; [`TokenStore`] persists
//! token hashes in `SQLite`. Shared SQL backends store both tables together and
//! join accounts on token lookups. [`AuthState::load`] selects the backend.
//!
//! Raw tokens are returned once; only their SHA-256 hashes are stored.

pub mod oidc;
pub mod schema;
pub(crate) mod validation;

pub use htpasswd::{
    is_bcrypt_hash, is_sha_hash, lock_htpasswd, parse_htpasswd, parse_htpasswd_line, verify_bcrypt,
    write_atomic,
};
pub use identify::identify;
pub use last_used::{LAST_USED_GRANULARITY_SECS, LastUsedWriter};
pub use owned_tokens::OwnedTokens;
pub use schema::CURRENT_SCHEMA_VERSION;
pub use token_store::{NewToken, TokenRecord, TokenStore};
pub use user_store::{ImportMode, PasswordCollisions, PasswordImport, UserStore};
pub use validation::MAX_PASSWORD_BYTES;

mod htpasswd;
use htpasswd::{dummy_verify, hash_bcrypt, verify_returning_user};

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

/// Public identity attributes of an active account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserAccount {
    pub id: String,
    pub username: String,
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

    /// Revoke every token `user_id` holds. Returns how many were revoked.
    pub async fn revoke_all_tokens(&self, user_id: &str) -> Result<usize> {
        self.tokens.revoke_all_for_user(user_id).await
    }

    /// Create an account with `username`, or change the existing account
    /// password. `None` means a concurrent writer removed the account.
    pub async fn put_user(&self, username: &str, password: &str) -> Result<Option<UserPut>> {
        match self.users.create_user(username, password).await? {
            UserCreation::Created => Ok(Some(UserPut::Created)),
            UserCreation::NameTaken => {
                let Some(id) = self.users.account_id(username).await? else {
                    return Ok(None);
                };
                match self.users.set_password(&id, password).await? {
                    PasswordChange::Changed => Ok(Some(UserPut::PasswordChanged)),
                    // Another caller removed the user between the two calls.
                    PasswordChange::NoSuchUser => Ok(None),
                }
            }
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
    /// file-backed otherwise. The fallible step (open the account /
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
            let shared = Arc::new(LibsqlAuth::connect(settings, auth.users.max_users).await?);
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
            let shared = Arc::new(PostgresAuth::connect(settings, auth.users.max_users).await?);
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
            let shared = Arc::new(MysqlAuth::connect(settings, auth.users.max_users).await?);
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
        let users = Arc::new(match auth.users.file.clone() {
            Some(path) => UserStore::open(path, auth.users.max_users)?,
            None => UserStore::in_memory_with_max_users(auth.users.max_users),
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

/// Immutable account identities and their password credentials. Clients reach it through
/// [`Self::add_or_login`] (npm `adduser` / `login`); the other operations
/// back the admin API. pnpr does not verify Basic credentials on requests,
/// so there is no per-request password check.
#[async_trait]
pub trait UserBackend: Send + Sync {
    /// Add a new user or verify a returning one. On success, returns
    /// the outcome plus the immutable account ID to bind follow-up
    /// token issuance to the same identity. A wrong password for an
    /// existing user is [`RegistryError::Unauthenticated`], and a new user
    /// past the registration cap is [`RegistryError::RegistrationDisabled`] /
    /// `TooManyUsers`.
    async fn add_or_login(&self, username: &str, password: &str)
    -> Result<(UpsertOutcome, String)>;

    /// Active account login names, sorted.
    async fn list_users(&self) -> Result<Vec<String>>;

    /// Active account IDs and login names, sorted by login name.
    async fn list_accounts(&self) -> Result<Vec<UserAccount>> {
        let mut accounts = Vec::new();
        for username in self.list_users().await? {
            if let Some(id) = self.account_id(&username).await? {
                accounts.push(UserAccount { id, username });
            }
        }
        Ok(accounts)
    }

    /// Active accounts matching the requested immutable IDs. Missing IDs are omitted.
    async fn accounts_by_id(&self, user_ids: &[String]) -> Result<Vec<UserAccount>> {
        let mut accounts = Vec::new();
        for id in user_ids {
            if let Some(username) = self.account_name(id).await? {
                accounts.push(UserAccount { id: id.clone(), username });
            }
        }
        Ok(accounts)
    }

    /// The credential hash of an active login name. Passwordless accounts
    /// contain [`NO_PASSWORD_HASH`]; missing or deleted accounts return `None`.
    async fn password_hash(&self, username: &str) -> Result<Option<String>>;

    /// Create `username` on an administrator's behalf. Not bound by the
    /// self-registration cap.
    async fn create_user(&self, username: &str, password: &str) -> Result<UserCreation>;

    /// Replace the password of the account identified by `user_id`.
    async fn set_password(&self, user_id: &str, password: &str) -> Result<PasswordChange>;

    /// Remove password credentials without changing the account ID or login name.
    async fn clear_password(&self, user_id: &str) -> Result<PasswordChange>;

    /// Disable the account identified by `user_id`, retaining its ID under a
    /// generated deleted name and freeing its login name. Never reassign the ID.
    async fn delete_user(&self, user_id: &str) -> Result<UserRemoval>;

    /// Provision a passwordless account. A taken login name is a conflict.
    async fn provision_user(&self, username: &str) -> Result<String>;

    /// Resolve an active login name to its immutable account ID.
    async fn account_id(&self, username: &str) -> Result<Option<String>>;

    /// Resolve an active account ID to its current login name.
    async fn account_name(&self, user_id: &str) -> Result<Option<String>>;

    /// Change an account's login name without changing its ID or grants.
    async fn rename_user(&self, user_id: &str, username: &str) -> Result<()>;

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

    /// Mint a fresh token for `user_id` with custom settings, persist its hash,
    /// and return the raw token and the stored record.
    async fn issue_token(
        &self,
        user_id: &str,
        new_token: NewToken,
    ) -> Result<(String, TokenRecord)>;

    /// Mint a default read-write token for `user_id`, persist its hash, and return
    /// the raw token. The raw token is never stored.
    async fn issue(&self, user_id: &str) -> Result<String> {
        let (raw, _) = self.issue_token(user_id, NewToken::default()).await?;
        Ok(raw)
    }

    /// Resolve a raw token back to its account ID. `Ok(None)` means the
    /// token was never issued, was revoked, or expired; `Err` means the backing
    /// store failed — never conflate the two, or a store outage reads as
    /// "not authenticated".
    async fn lookup(&self, raw: &str) -> Result<Option<String>>;

    /// Resolve a raw token to its full record — the account ID plus the
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

    /// All tokens owned by `user_id`, as `(key, record)` pairs where
    /// `key` is the SHA-256 hex digest the listing endpoint surfaces.
    async fn list_for_user(&self, user_id: &str) -> Result<Vec<(String, TokenRecord)>>;

    /// Remove a token by its key (the SHA-256 hex digest). Returns the
    /// deleted record so a higher layer can confirm the revocation.
    async fn revoke_by_key(&self, key: &str) -> Result<Option<TokenRecord>>;

    /// Remove a token by its raw value — the `DELETE /-/user/token/:tok`
    /// (npm logout) path puts the bearer token verbatim in the URL, so
    /// this hashes first and defers to [`Self::revoke_by_key`].
    async fn revoke_by_raw(&self, raw: &str) -> Result<Option<TokenRecord>> {
        self.revoke_by_key(&sha256_hex(raw.as_bytes())).await
    }

    /// Remove every token `user_id` holds. Returns how many were removed.
    async fn revoke_all_for_user(&self, user_id: &str) -> Result<usize> {
        let mut revoked = 0;
        for (key, _) in &self.list_for_user(user_id).await? {
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

/// The outcome of removing an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserRemoval {
    /// The account is gone.
    Removed,
    /// There was no such account.
    NoSuchUser,
}

/// SQL account records, keyed by immutable ID.
#[cfg(any(feature = "backend-libsql", feature = "backend-postgres", feature = "backend-mysql"))]
pub(crate) const USERS_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS users (
    id           VARCHAR(64) PRIMARY KEY,
    username     VARCHAR(255) UNIQUE NOT NULL,
    bcrypt_hash  TEXT NOT NULL
)";

#[cfg(any(feature = "backend-libsql", feature = "backend-postgres", feature = "backend-mysql"))]
pub(crate) const AUTH_COUNTERS_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS auth_counters (
    name   VARCHAR(64) PRIMARY KEY,
    value  BIGINT NOT NULL
)";

#[cfg(test)]
mod retirement_tests;
#[cfg(test)]
mod tests;

/// A deleted account credential that cannot authenticate.
pub const DELETED_USER_HASH: &str = "!";

/// Generate an opaque identity independently of any reusable name.
#[must_use]
pub fn new_principal_id() -> String {
    sha256_hex(&fresh_secret())
}

/// An account whose credentials are managed by an identity provider.
pub const NO_PASSWORD_HASH: &str = "*";
