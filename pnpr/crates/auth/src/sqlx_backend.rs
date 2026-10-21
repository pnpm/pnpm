//! Shared SQL auth backend for relational databases supported through
//! feature-gated `sqlx` drivers.

#[cfg(feature = "backend-postgres")]
pub(super) mod postgres;

#[cfg(feature = "backend-mysql")]
pub(super) mod mysql;

use super::{
    DEFAULT_BCRYPT_COST, NewToken, PasswordChange, TokenBackend, TokenRecord, UpsertOutcome,
    UserBackend, UserCreation, UserRemoval, dummy_verify, fresh_secret, hash_bcrypt, is_sha_hash,
    sha256_hex,
    token_store::{mint_token, unix_seconds},
    verify_returning_user, with_auth_timeout,
};
use crate::validation::validate_username;
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
        if self.next_cap_reconcile_at
            .compare_exchange(next, updated_next, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return Ok(false);
        }
        self.db.reconcile_user_counter_overcount().await
    }

    async fn check_db_readiness(&self) -> Result<()>
    where
        Db: AuthSqlBackend,
    {
        with_auth_timeout(self.timeout, self.db.check_readiness()).await
    }

    async fn verify_and_maybe_upgrade(
        &self,
        stored: StoredUser,
        password: &str,
    ) -> Result<(UpsertOutcome, String)>
    where
        Db: AuthSqlBackend,
    {
        let (outcome, user) = verify_returning_user(
            &stored.username,
            password,
            stored.bcrypt_hash.clone(),
            DEFAULT_BCRYPT_COST,
        )
        .await?;
        if is_sha_hash(&stored.bcrypt_hash) {
            let new_hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
            with_auth_timeout(
                self.timeout,
                self.db.update_user_hash_if_current(
                    &stored.username,
                    &stored.bcrypt_hash,
                    &new_hash,
                ),
            )
            .await?;
        }
        Ok((outcome, user))
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
    async fn touch_token(&self, _token_hash: &str, _now: u64) -> Result<()> {
        Ok(())
    }
    async fn find_token(&self, token_hash: &str) -> Result<Option<TokenRecord>>;
    /// [`Self::find_token`] for a token whose owner is a stored user. The
    /// databases answer it in one joined query; this default takes two.
    async fn find_owned_token(&self, token_hash: &str) -> Result<Option<TokenRecord>> {
        let Some(record) = self.find_token(token_hash).await? else {
            return Ok(None);
        };
        Ok(self.stored_user(&record.username).await?.map(|_| record))
    }
    async fn list_tokens(&self, username: &str) -> Result<Vec<(String, TokenRecord)>>;
    async fn delete_token(&self, token_hash: &str) -> Result<()>;
    async fn list_usernames(&self) -> Result<Vec<String>>;
    async fn update_password_hash(
        &self,
        username: &str,
        bcrypt_hash: &str,
    ) -> Result<PasswordChange>;
    async fn update_user_hash_if_current(
        &self,
        _username: &str,
        _old_hash: &str,
        _new_hash: &str,
    ) -> Result<bool> {
        Ok(false)
    }
    /// Remove the user and take one off the `users` counter in one transaction.
    async fn delete_user(&self, username: &str) -> Result<UserRemoval>;
    async fn check_readiness(&self) -> Result<()> {
        Ok(())
    }
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
            return self.verify_and_maybe_upgrade(stored, password).await;
        }

        if matches!(self.max_users, MaxUsers::Disabled) {
            dummy_verify(password, DEFAULT_BCRYPT_COST).await;
            return Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") });
        }

        self.check_registration_capacity().await?;

        let hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
        match self.db.insert_user(username, &hash, self.max_users).await? {
            InsertUser::Created => Ok((UpsertOutcome::Created, username.to_string())),
            InsertUser::Existing(stored) => self.verify_and_maybe_upgrade(stored, password).await,
            InsertUser::CapReached => match self.max_users {
                MaxUsers::Limited(max) => Err(RegistryError::TooManyUsers { max }),
                MaxUsers::Disabled | MaxUsers::Unlimited => {
                    Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") })
                }
            },
        }
    }

    async fn list_users(&self) -> Result<Vec<String>> {
        with_auth_timeout(self.timeout, self.db.list_usernames()).await
    }

    async fn password_hash(&self, username: &str) -> Result<Option<String>> {
        let stored = with_auth_timeout(self.timeout, self.db.stored_user(username)).await?;
        Ok(stored.map(|stored| stored.bcrypt_hash))
    }

    /// Inserts as an uncapped store would, so the `users` counter still
    /// counts the new user.
    async fn create_user(&self, username: &str, password: &str) -> Result<UserCreation> {
        validate_username(username)?;
        let hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
        match self.db.insert_user(username, &hash, MaxUsers::Unlimited).await? {
            InsertUser::Created => Ok(UserCreation::Created),
            // An uncapped insert cannot reach the cap.
            InsertUser::Existing(_) | InsertUser::CapReached => Ok(UserCreation::NameTaken),
        }
    }

    async fn set_password(&self, username: &str, password: &str) -> Result<PasswordChange> {
        let hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
        self.db.update_password_hash(username, &hash).await
    }

    async fn delete_user(&self, username: &str) -> Result<UserRemoval> {
        self.db.delete_user(username).await
    }

    async fn check_readiness(&self) -> Result<()> {
        self.check_db_readiness().await
    }
}

#[async_trait]
impl<Db> TokenBackend for SqlAuth<Db>
where
    Db: AuthSqlBackend,
{
    async fn issue_token(
        &self,
        username: &str,
        new_token: NewToken,
    ) -> Result<(String, TokenRecord)> {
        let nonce = self.counter.fetch_add(1, Ordering::Relaxed);
        let raw = mint_token(&self.secret, nonce, username);
        let token_hash = sha256_hex(raw.as_bytes());
        let now = unix_seconds();
        let record = TokenRecord {
            username: username.to_string(),
            created_at: now,
            last_used_at: now,
            readonly: new_token.readonly,
            cidr_whitelist: new_token.cidr_whitelist,
            name: new_token.name,
            expires_at: new_token.expires_at,
            scopes: new_token.scopes,
            kind: new_token.kind,
        };
        self.db.insert_token(&token_hash, &record).await?;
        Ok((raw, record))
    }

    async fn issue(&self, username: &str) -> Result<String> {
        let (raw, _) = self.issue_token(username, NewToken::default()).await?;
        Ok(raw)
    }

    async fn touch_key(&self, key: &str, now: u64) -> Result<()> {
        with_auth_timeout(self.timeout, self.db.touch_token(key, now)).await
    }

    async fn lookup(&self, raw: &str) -> Result<Option<String>> {
        let token_hash = sha256_hex(raw.as_bytes());
        let record = with_auth_timeout(self.timeout, self.db.find_owned_token(&token_hash)).await?;
        let now = unix_seconds();
        match record {
            Some(r) if r.expires_at.is_none_or(|exp| exp > now) => Ok(Some(r.username)),
            _ => Ok(None),
        }
    }

    async fn lookup_record(&self, raw: &str) -> Result<Option<TokenRecord>> {
        let token_hash = sha256_hex(raw.as_bytes());
        let record = with_auth_timeout(self.timeout, self.db.find_owned_token(&token_hash)).await?;
        let now = unix_seconds();
        match record {
            Some(r) if r.expires_at.is_none_or(|exp| exp > now) => Ok(Some(r)),
            _ => Ok(None),
        }
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

    async fn check_readiness(&self) -> Result<()> {
        self.check_db_readiness().await
    }
}

fn invalid_pool_size(backend: &str) -> RegistryError {
    RegistryError::InvalidConfig {
        reason: format!("backend.{backend}.maxConnections must be greater than 0"),
    }
}

fn sql_max_users(max: u64, backend: &str) -> Result<i64> {
    i64::try_from(max)
        .map_err(|_| RegistryError::InvalidConfig {
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
