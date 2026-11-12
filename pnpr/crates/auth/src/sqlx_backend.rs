//! Shared SQL auth backend for relational databases supported through
//! feature-gated `sqlx` drivers.

#[cfg(feature = "backend-postgres")]
pub(super) mod postgres;

#[cfg(feature = "backend-mysql")]
pub(super) mod mysql;

use super::{
    AccountCredential, AccountStatus, DEFAULT_BCRYPT_COST, PasswordChange, TokenRecord,
    UpsertOutcome, UserBackend, UserCreation, UserRemoval,
    account::{decode_sql_account_state, deprovisioned_login_error},
    dummy_verify, hash_bcrypt, is_sha_hash,
    token_store::unix_millis,
    validation::token_timestamp_to_sql,
    verify_returning_user, with_auth_timeout,
};
mod request_pool;
mod tokens;
mod users;
use request_pool::RequestPool;

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
    next_cap_reconcile_at: AtomicU64,
    max_users: MaxUsers,
    timeout: Duration,
}

const CAP_RECONCILE_INTERVAL_MILLIS: u64 = 60_000;

impl<Db> SqlAuth<Db> {
    fn new(db: Db, max_users: MaxUsers, timeout: Duration) -> Self {
        Self { db, next_cap_reconcile_at: AtomicU64::new(0), max_users, timeout }
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
        let now = unix_millis();
        let next = self.next_cap_reconcile_at.load(Ordering::Relaxed);
        if now < next {
            return Ok(false);
        }
        let updated_next = now.saturating_add(CAP_RECONCILE_INTERVAL_MILLIS);
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
        if stored.status == AccountStatus::Deactivated {
            return Err(deprovisioned_login_error(&stored.username));
        }
        if stored.status.is_deleted() {
            dummy_verify(password, DEFAULT_BCRYPT_COST).await;
            return Err(RegistryError::Unauthenticated {
                resource: format!("user {:?}", stored.username),
            });
        }
        let (outcome, _) = verify_returning_user(
            &stored.username,
            password,
            &stored.credential,
            DEFAULT_BCRYPT_COST,
        )
        .await?;
        if let AccountCredential::PasswordHash(ref old_hash) = stored.credential
            && is_sha_hash(old_hash)
        {
            let new_hash = hash_bcrypt(password.to_string(), DEFAULT_BCRYPT_COST).await?;
            with_auth_timeout(
                self.timeout,
                self.db.update_user_hash_if_current(&stored.id, old_hash, &new_hash),
            )
            .await?;
        }
        Ok((outcome, stored.id))
    }
}

#[async_trait]
trait AuthSqlBackend: Send + Sync {
    async fn stored_user(&self, username: &str) -> Result<Option<StoredUser>>;
    async fn account_name(&self, user_id: &str) -> Result<Option<String>>;
    async fn active_account_name(&self, user_id: &str) -> Result<Option<String>> {
        if self.account_status(user_id).await? != Some(AccountStatus::Active) {
            return Ok(None);
        }
        self.account_name(user_id).await
    }
    async fn account_status(&self, user_id: &str) -> Result<Option<AccountStatus>> {
        Ok(self.account_name(user_id).await?.map(|_| AccountStatus::Active))
    }
    async fn status_revisions(
        &self,
        user_ids: &[String],
    ) -> Result<std::collections::HashMap<String, crate::AccountStatusRevision>> {
        let mut revisions = std::collections::HashMap::new();
        for id in user_ids {
            if let Some(status) = self.account_status(id).await? {
                revisions.insert(id.clone(), crate::AccountStatusRevision { status, revision: 0 });
            }
        }
        Ok(revisions)
    }
    async fn apply_account_status(
        &self,
        _user_id: &str,
        _revision: u64,
        _lifecycle: crate::AccountLifecycle,
    ) -> Result<crate::AccountStatusOutcome> {
        Err(RegistryError::BadRequest {
            reason: "Account status changes are not supported by this account backend".to_string(),
        })
    }
    async fn account_login(&self, _user_id: &str) -> Result<Option<crate::AccountLogin>> {
        Err(RegistryError::BadRequest {
            reason: "Conditional account renames are not supported by this account backend"
                .to_string(),
        })
    }
    async fn rename_user(&self, user_id: &str, username: &str) -> Result<()>;
    async fn rename_user_if_current(
        &self,
        _user_id: &str,
        _expected_revision: u64,
        _username: &str,
    ) -> Result<crate::UserRename> {
        Err(RegistryError::BadRequest {
            reason: "Conditional account renames are not supported by this account backend"
                .to_string(),
        })
    }
    async fn list_accounts(&self) -> Result<Vec<crate::UserAccount>> {
        let mut accounts = Vec::new();
        for username in self.list_usernames().await? {
            if let Some(user) = self.stored_user(&username).await?
                && !user.status.is_deleted()
            {
                accounts.push(crate::UserAccount { id: user.id, username: user.username });
            }
        }
        Ok(accounts)
    }
    async fn accounts_by_id(&self, user_ids: &[String]) -> Result<Vec<crate::UserAccount>> {
        let mut accounts = Vec::new();
        for id in user_ids {
            if let Some(username) = self.account_name(id).await? {
                accounts.push(crate::UserAccount { id: id.clone(), username });
            }
        }
        Ok(accounts)
    }
    async fn user_count(&self) -> Result<u64>;
    async fn reconcile_user_counter_overcount(&self) -> Result<bool>;
    async fn insert_user(
        &self,
        username: &str,
        password_hash: Option<&str>,
        max_users: MaxUsers,
    ) -> Result<InsertUser>;
    async fn insert_token(&self, token_hash: &str, record: &TokenRecord) -> Result<()>;
    async fn touch_token(&self, _token_hash: &str, _now: u64) -> Result<()> {
        Ok(())
    }
    async fn find_token(&self, token_hash: &str) -> Result<Option<TokenRecord>>;
    /// [`Self::find_token`] for a token whose owner is an active stored user.
    /// The databases answer it in one joined query; this default takes two.
    async fn find_owned_token(&self, token_hash: &str) -> Result<Option<TokenRecord>> {
        let Some(record) = self.find_token(token_hash).await? else {
            return Ok(None);
        };
        Ok(self
            .active_account_name(&record.user_id)
            .await?
            .map(|name| record.with_owner_name(name)))
    }
    async fn list_tokens(&self, user_id: &str) -> Result<Vec<(String, TokenRecord)>>;
    async fn delete_token(&self, token_hash: &str) -> Result<()>;
    async fn delete_user_tokens(&self, _user_id: &str) -> Result<u64> {
        Ok(0)
    }
    async fn delete_expired_tokens(&self, _now: u64) -> Result<u64> {
        Ok(0)
    }
    async fn list_usernames(&self) -> Result<Vec<String>>;
    async fn update_password_hash(
        &self,
        user_id: &str,
        password_hash: Option<&str>,
    ) -> Result<PasswordChange>;
    async fn update_user_hash_if_current(
        &self,
        _username: &str,
        _old_hash: &str,
        _new_hash: &str,
    ) -> Result<bool> {
        Ok(false)
    }
    async fn delete_user(&self, user_id: &str) -> Result<UserRemoval>;

    async fn check_readiness(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
struct StoredUser {
    id: String,
    username: String,
    status: AccountStatus,
    credential: AccountCredential,
}

enum InsertUser {
    Created(String),
    Existing(StoredUser),
    CapReached,
    Confusable,
}

fn sql_max_users(max: u64, backend: &str) -> Result<i64> {
    i64::try_from(max)
        .map_err(|_| RegistryError::InvalidConfig {
            reason: format!("backend.{backend} auth max_users must fit a signed BIGINT"),
        })
}

#[cfg(test)]
mod tests;
