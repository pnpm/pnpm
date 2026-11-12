use super::{
    super::{AccountStatus, CredentialKind, PasswordChange, TokenRecord, UserRemoval},
    AuthSqlBackend, InsertUser, RequestPool, SqlAuth, StoredUser, decode_sql_account_state,
    sql_max_users, token_timestamp_to_sql, unix_millis, with_auth_timeout,
};
use async_trait::async_trait;
use pnpr_config::{MaxUsers, SqlBackendSettings};
use pnpr_error::{RegistryError, Result};
use sqlx::{PgConnection, Row};

#[derive(Debug)]
pub(crate) struct PostgresDatabase {
    pool: RequestPool<sqlx::Postgres>,
}

mod accounts;
mod tokens;

pub(crate) type PostgresAuth = SqlAuth<PostgresDatabase>;

impl SqlAuth<PostgresDatabase> {
    pub(crate) async fn connect(
        settings: &SqlBackendSettings,
        max_users: MaxUsers,
    ) -> Result<Self> {
        let startup_options = pnpr_sql::postgres_options(
            settings,
            settings.startup_timeout,
            settings.startup_timeout,
        )?;
        let startup_pool = std::sync::Arc::new(startup_options.connect_lazy(&settings.url)?);
        let connection =
            pnpr_sql::connect_startup(&startup_pool, &settings.url, settings.startup_timeout)
                .await?;
        let startup = std::sync::Arc::new(tokio::sync::Mutex::new(Some(connection)));
        let startup_db = PostgresDatabase {
            pool: RequestPool::with_startup(
                std::sync::Arc::clone(&startup_pool),
                std::sync::Arc::clone(&startup),
                settings.startup_timeout,
            ),
        };
        with_auth_timeout(settings.startup_timeout, async {
            pnpr_sql::network::ensure_postgres_schema(
                std::sync::Arc::clone(&startup_pool),
                startup,
                settings.startup_timeout,
                "PostgreSQL auth database",
            )
            .await?;
            startup_db.ensure_user_counter().await
        })
        .await?;
        startup_pool.close().await;

        let pool = pnpr_sql::postgres_pool(settings)?;
        let db = PostgresDatabase { pool: RequestPool::new(pool, settings.timeout) };
        Ok(SqlAuth::new(db, max_users, settings.timeout))
    }
}

#[async_trait]
impl AuthSqlBackend for PostgresDatabase {
    async fn stored_user(&self, username: &str) -> Result<Option<StoredUser>> {
        self.stored_user_impl(username).await
    }

    async fn account_name(&self, user_id: &str) -> Result<Option<String>> {
        Ok(sqlx::query_scalar("SELECT username FROM users WHERE id = $1 AND status <> 'deleted'")
            .bind(user_id)
            .fetch_optional(&mut *self.pool.acquire().await?)
            .await?)
    }

    async fn active_account_name(&self, user_id: &str) -> Result<Option<String>> {
        Ok(sqlx::query_scalar("SELECT username FROM users WHERE id = $1 AND status = 'active'")
            .bind(user_id)
            .fetch_optional(&mut *self.pool.acquire().await?)
            .await?)
    }

    async fn account_status(&self, user_id: &str) -> Result<Option<AccountStatus>> {
        self.account_status_impl(user_id).await
    }

    async fn apply_account_status(
        &self,
        user_id: &str,
        revision: u64,
        lifecycle: crate::AccountLifecycle,
    ) -> Result<crate::AccountStatusOutcome> {
        self.apply_account_status_impl(user_id, revision, lifecycle).await
    }

    async fn account_login(&self, user_id: &str) -> Result<Option<crate::AccountLogin>> {
        self.account_login_impl(user_id).await
    }

    async fn rename_user(&self, user_id: &str, username: &str) -> Result<()> {
        self.rename_account(user_id, username).await
    }

    async fn rename_user_if_current(
        &self,
        user_id: &str,
        expected_revision: u64,
        username: &str,
    ) -> Result<crate::UserRename> {
        self.rename_account_if_current(user_id, expected_revision, username).await
    }

    async fn list_accounts(&self) -> Result<Vec<crate::UserAccount>> {
        let rows = sqlx::query(
            "SELECT id, username FROM users WHERE status <> 'deleted' ORDER BY username",
        )
        .fetch_all(&mut *self.pool.acquire().await?)
        .await?;
        rows.into_iter()
            .map(|row| Ok(crate::UserAccount { id: row.try_get(0)?, username: row.try_get(1)? }))
            .collect()
    }

    async fn accounts_by_id(&self, user_ids: &[String]) -> Result<Vec<crate::UserAccount>> {
        self.accounts_by_id_impl(user_ids).await
    }

    async fn user_count(&self) -> Result<u64> {
        let Some(count) = self.user_counter().await? else {
            self.ensure_user_counter().await?;
            return Ok(self
                .user_counter()
                .await?
                .unwrap_or(0)
                .max(0) as u64);
        };
        Ok(count.max(0) as u64)
    }

    async fn reconcile_user_counter_overcount(&self) -> Result<bool> {
        self.reconcile_user_counter_overcount_impl().await
    }

    async fn insert_user(
        &self,
        username: &str,
        password_hash: Option<&str>,
        max_users: MaxUsers,
    ) -> Result<InsertUser> {
        let kind = CredentialKind::for_optional_hash(password_hash).as_str();
        let Some(mut tx) = self.claim_tx_for_insert(max_users).await? else {
            return self.existing_or_cap_reached(username).await;
        };
        let id = crate::new_principal_id();
        let created_at = token_timestamp_to_sql(unix_millis());
        let inserted = sqlx::query(
            "INSERT INTO users (id, username, username_skeleton_hash, status, credential_kind, password_hash, created_at)
             VALUES ($1, $2, $3, 'active', $4, $5, $6)",
        )
        .bind(&id)
        .bind(username)
        .bind(crate::validation::username_skeleton_hash(username))
        .bind(kind)
        .bind(password_hash)
        .bind(created_at)
        .execute(&mut *tx)
        .await;
        match inserted {
            Ok(_) => {
                tx.commit().await?;
                Ok(InsertUser::Created(id))
            }
            Err(err) if is_unique_violation(&err) => {
                tx.rollback().await?;
                match self.stored_user(username).await? {
                    Some(stored) => Ok(InsertUser::Existing(stored)),
                    None => Ok(InsertUser::Confusable),
                }
            }
            Err(err) => Err(err.into()),
        }
    }

    async fn insert_token(&self, token_hash: &str, record: &TokenRecord) -> Result<()> {
        self.insert_token_impl(token_hash, record).await
    }

    async fn touch_token(&self, token_hash: &str, now: u64) -> Result<()> {
        self.touch_token_impl(token_hash, now).await
    }

    async fn find_token(&self, token_hash: &str) -> Result<Option<TokenRecord>> {
        self.find_token_impl(token_hash).await
    }

    async fn find_owned_token(&self, token_hash: &str) -> Result<Option<TokenRecord>> {
        self.find_owned_token_impl(token_hash).await
    }

    async fn list_tokens(&self, user_id: &str) -> Result<Vec<(String, TokenRecord)>> {
        self.list_tokens_impl(user_id).await
    }

    async fn delete_token(&self, token_hash: &str) -> Result<()> {
        self.delete_token_impl(token_hash).await
    }

    async fn delete_user_tokens(&self, user_id: &str) -> Result<u64> {
        self.delete_user_tokens_impl(user_id).await
    }

    async fn delete_expired_tokens(&self, now: u64) -> Result<u64> {
        self.delete_expired_tokens_impl(now).await
    }

    async fn list_usernames(&self) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT username FROM users WHERE status <> 'deleted' ORDER BY username",
        )
        .fetch_all(&mut *self.pool.acquire().await?)
        .await?)
    }

    async fn update_password_hash(
        &self,
        user_id: &str,
        password_hash: Option<&str>,
    ) -> Result<PasswordChange> {
        let kind = match password_hash {
            Some(hash) => CredentialKind::from_hash(hash)
                .expect("update_password_hash requires a supported hash")
                .as_str(),
            None => CredentialKind::None.as_str(),
        };
        let updated = sqlx::query(
            "UPDATE users SET credential_kind = $2, password_hash = $3
             WHERE id = $1 AND status <> 'deleted'",
        )
        .bind(user_id)
        .bind(kind)
        .bind(password_hash)
        .execute(&mut *self.pool.acquire().await?)
        .await?;
        if updated.rows_affected() > 0 {
            Ok(PasswordChange::Changed)
        } else {
            Ok(PasswordChange::NoSuchUser)
        }
    }

    async fn update_user_hash_if_current(
        &self,
        user_id: &str,
        old_hash: &str,
        new_hash: &str,
    ) -> Result<bool> {
        let kind = CredentialKind::from_hash(new_hash)
            .expect("update_user_hash_if_current requires a supported hash")
            .as_str();
        let updated = sqlx::query(
            "UPDATE users SET credential_kind = $3, password_hash = $4
             WHERE id = $1 AND password_hash = $2 AND status = 'active'",
        )
        .bind(user_id)
        .bind(old_hash)
        .bind(kind)
        .bind(new_hash)
        .execute(&mut *self.pool.acquire().await?)
        .await?;
        Ok(updated.rows_affected() > 0)
    }

    async fn delete_user(&self, user_id: &str) -> Result<UserRemoval> {
        self.remove_user_record(user_id).await
    }

    async fn check_readiness(&self) -> Result<()> {
        sqlx::query("SELECT 1;")
            .execute(&mut *self.pool.acquire().await?)
            .await
            .map_err(RegistryError::from)?;
        Ok(())
    }
}

impl PostgresDatabase {
    async fn remove_user_record(&self, user_id: &str) -> Result<UserRemoval> {
        let mut tx = self.pool.begin().await?;
        let statement = "UPDATE users
                         SET username = 'deleted:' || id,
                             username_skeleton_hash = NULL,
                             status = 'deleted',
                             credential_kind = 'none',
                             password_hash = NULL
                         WHERE id = $1 AND status <> 'deleted'";
        let removed = sqlx::query(statement)
            .bind(user_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if removed > 0 {
            sqlx::query("UPDATE auth_counters SET value = value - 1 WHERE name = $1 AND value > 0")
                .bind("users")
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(if removed > 0 { UserRemoval::Removed } else { UserRemoval::NoSuchUser })
    }

    async fn ensure_user_counter(&self) -> Result<()> {
        let count = self.actual_user_count().await?;
        if self.set_user_counter_floor(count).await? > 0 {
            return Ok(());
        }
        let inserted = sqlx::query("INSERT INTO auth_counters (name, value) VALUES ($1, $2)")
            .bind("users")
            .bind(count)
            .execute(&mut *self.pool.acquire().await?)
            .await;
        match inserted {
            Ok(_) => Ok(()),
            Err(err) if is_unique_violation(&err) => {
                self.set_user_counter_floor(count).await?;
                Ok(())
            }
            Err(err) => Err(err.into()),
        }
    }

    async fn actual_user_count(&self) -> Result<i64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE status <> 'deleted'")
            .fetch_one(&mut *self.pool.acquire().await?)
            .await?;
        Ok(count.max(0))
    }

    async fn user_counter(&self) -> Result<Option<i64>> {
        let count: Option<i64> =
            sqlx::query_scalar("SELECT value FROM auth_counters WHERE name = $1")
                .bind("users")
                .fetch_optional(&mut *self.pool.acquire().await?)
                .await?;
        Ok(count)
    }

    async fn set_user_counter_floor(&self, count: i64) -> Result<u64> {
        let updated = sqlx::query(
            "UPDATE auth_counters
             SET value = CASE WHEN value < $2 THEN $2 ELSE value END
             WHERE name = $1",
        )
        .bind("users")
        .bind(count)
        .execute(&mut *self.pool.acquire().await?)
        .await?;
        Ok(updated.rows_affected())
    }

    async fn reconcile_user_counter_overcount_impl(&self) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let Some(counter): Option<i64> =
            sqlx::query_scalar("SELECT value FROM auth_counters WHERE name = $1 FOR UPDATE")
                .bind("users")
                .fetch_optional(&mut *tx)
                .await?
        else {
            tx.commit().await?;
            return Ok(false);
        };
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE status <> 'deleted'")
            .fetch_one(&mut *tx)
            .await?;
        if counter <= count {
            tx.commit().await?;
            return Ok(false);
        }
        sqlx::query("UPDATE auth_counters SET value = $2 WHERE name = $1")
            .bind("users")
            .bind(count.max(0))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    async fn existing_or_cap_reached(&self, username: &str) -> Result<InsertUser> {
        match self.stored_user(username).await? {
            Some(stored) => Ok(InsertUser::Existing(stored)),
            None => Ok(InsertUser::CapReached),
        }
    }
}

fn is_unique_violation(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .is_some_and(|code| code.as_ref() == "23505")
}

async fn claim_user_slot(connection: &mut PgConnection, max_users: MaxUsers) -> Result<bool> {
    match max_users {
        MaxUsers::Limited(max) => {
            let max = sql_max_users(max, "postgres")?;
            let updated = sqlx::query(
                "UPDATE auth_counters SET value = value + 1
                 WHERE name = $1 AND value < $2",
            )
            .bind("users")
            .bind(max)
            .execute(connection)
            .await?;
            Ok(updated.rows_affected() != 0)
        }
        MaxUsers::Unlimited => {
            sqlx::query("UPDATE auth_counters SET value = value + 1 WHERE name = $1")
                .bind("users")
                .execute(connection)
                .await?;
            Ok(true)
        }
        MaxUsers::Disabled => Ok(true),
    }
}

#[cfg(test)]
mod on_postgres;
