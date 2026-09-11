//! Shared SQL auth backend for relational databases supported through
//! feature-gated `sqlx` drivers.

#[cfg(feature = "backend-postgres")]
pub(super) mod postgres;

#[cfg(feature = "backend-mysql")]
pub(super) mod mysql;

use super::{
    DEFAULT_BCRYPT_COST, TokenBackend, TokenRecord, UpsertOutcome, UserBackend, fresh_secret,
    hash_bcrypt, sha256_hex,
    token_store::{mint_token, unix_seconds},
    validate_username, verify_returning_user, with_auth_timeout,
};
use async_trait::async_trait;
use pnpr_config::MaxUsers;
use pnpr_error::{RegistryError, Result};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

#[derive(Debug)]
pub(crate) struct SqlAuth<Db> {
    db: Db,
    secret: [u8; 32],
    counter: AtomicU64,
    next_cap_reconcile_at: AtomicU64,
    max_users: MaxUsers,
    timeout: Duration,
}

const CAP_RECONCILE_INTERVAL_SECS: u64 = 60;

impl<Db> SqlAuth<Db> {
    fn new(db: Db, max_users: MaxUsers, timeout: Duration) -> Self {
        Self {
            db,
            secret: fresh_secret(),
            counter: AtomicU64::new(0),
            next_cap_reconcile_at: AtomicU64::new(0),
            max_users,
            timeout,
        }
    }

    async fn check_registration_capacity(&self) -> Result<()>
    where
        Db: AuthSqlBackend,
    {
        let max = match self.max_users {
            MaxUsers::Disabled => return Err(RegistryError::RegistrationDisabled),
            MaxUsers::Unlimited => return Ok(()),
            MaxUsers::Limited(max) => max,
        };
        if with_auth_timeout(self.timeout, self.db.user_count()).await? < max {
            return Ok(());
        }
        if self.reconcile_capped_counter_once_per_interval().await?
            && with_auth_timeout(self.timeout, self.db.user_count()).await? < max
        {
            return Ok(());
        }
        Err(RegistryError::TooManyUsers { max })
    }

    async fn reconcile_capped_counter_once_per_interval(&self) -> Result<bool>
    where
        Db: AuthSqlBackend,
    {
        let now = unix_seconds();
        let next = self.next_cap_reconcile_at.load(Ordering::Relaxed);
        if now < next {
            return Ok(false);
        }
        let updated_next = now.saturating_add(CAP_RECONCILE_INTERVAL_SECS);
        if self
            .next_cap_reconcile_at
            .compare_exchange(next, updated_next, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return Ok(false);
        }
        self.db.reconcile_user_counter_overcount().await
    }
}

#[async_trait]
trait AuthSqlBackend: Send + Sync {
    async fn stored_user(&self, username: &str) -> Result<Option<StoredUser>>;
    async fn user_count(&self) -> Result<u64>;
    async fn reconcile_user_counter_overcount(&self) -> Result<bool>;
    async fn insert_user(
        &self,
        username: &str,
        bcrypt_hash: &str,
        max_users: MaxUsers,
    ) -> Result<InsertUser>;
    async fn insert_token(&self, token_hash: &str, record: &TokenRecord) -> Result<()>;
    async fn lookup_token(&self, token_hash: &str) -> Result<Option<String>>;
    async fn find_token(&self, token_hash: &str) -> Result<Option<TokenRecord>>;
    async fn list_tokens(&self, username: &str) -> Result<Vec<(String, TokenRecord)>>;
    async fn delete_token(&self, token_hash: &str) -> Result<()>;
}

#[derive(Clone)]
struct StoredUser {
    username: String,
    bcrypt_hash: String,
}

enum InsertUser {
    Created,
    Existing(StoredUser),
    CapReached,
}

#[async_trait]
impl<Db> UserBackend for SqlAuth<Db>
where
    Db: AuthSqlBackend,
{
    async fn add_or_login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(UpsertOutcome, String)> {
        validate_username(username)?;

        if let Some(stored) = with_auth_timeout(self.timeout, self.db.stored_user(username)).await?
        {
            return verify_returning_user(&stored.username, password, stored.bcrypt_hash).await;
        }

        self.check_registration_capacity().await?;

        let hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
        match self.db.insert_user(username, &hash, self.max_users).await? {
            InsertUser::Created => Ok((UpsertOutcome::Created, username.to_string())),
            InsertUser::Existing(stored) => {
                verify_returning_user(&stored.username, password, stored.bcrypt_hash).await
            }
            InsertUser::CapReached => match self.max_users {
                MaxUsers::Limited(max) => Err(RegistryError::TooManyUsers { max }),
                MaxUsers::Disabled | MaxUsers::Unlimited => {
                    Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") })
                }
            },
        }
    }
}

#[async_trait]
impl<Db> TokenBackend for SqlAuth<Db>
where
    Db: AuthSqlBackend,
{
    async fn issue(&self, username: &str) -> Result<String> {
        let nonce = self.counter.fetch_add(1, Ordering::Relaxed);
        let raw = mint_token(&self.secret, nonce, username);
        let token_hash = sha256_hex(raw.as_bytes());
        let now = unix_seconds();
        let record = TokenRecord {
            username: username.to_string(),
            created_at: now,
            last_used_at: now,
            readonly: false,
            cidr_whitelist: Vec::new(),
        };
        self.db.insert_token(&token_hash, &record).await?;
        Ok(raw)
    }

    async fn lookup(&self, raw: &str) -> Result<Option<String>> {
        let token_hash = sha256_hex(raw.as_bytes());
        with_auth_timeout(self.timeout, self.db.lookup_token(&token_hash)).await
    }

    async fn find_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        with_auth_timeout(self.timeout, self.db.find_token(key)).await
    }

    async fn list_for_user(&self, username: &str) -> Result<Vec<(String, TokenRecord)>> {
        with_auth_timeout(self.timeout, self.db.list_tokens(username)).await
    }

    async fn revoke_by_key(&self, key: &str) -> Result<Option<TokenRecord>> {
        let Some(record) = with_auth_timeout(self.timeout, self.db.find_token(key)).await? else {
            return Ok(None);
        };
        self.db.delete_token(key).await?;
        Ok(Some(record))
    }
}

fn invalid_pool_size(backend: &str) -> RegistryError {
    RegistryError::InvalidConfig {
        reason: format!("backend.{backend}.maxConnections must be greater than 0"),
    }
}

fn sql_max_users(max: u64, backend: &str) -> Result<i64> {
    i64::try_from(max).map_err(|_| RegistryError::InvalidConfig {
        reason: format!("backend.{backend} auth max_users must fit a signed BIGINT"),
    })
}

#[cfg(any(feature = "backend-postgres", feature = "backend-mysql"))]
fn timeout_millis(timeout: Duration) -> u128 {
    timeout.as_millis().max(1)
}

#[cfg(feature = "backend-mysql")]
fn timeout_seconds(timeout: Duration) -> u64 {
    timeout.as_secs().max(1)
}

#[cfg(test)]
mod tests;
