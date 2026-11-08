use super::{
    super::{PasswordChange, TokenRecord, UserRemoval},
    AuthSqlBackend, InsertUser, SqlAuth, StoredUser, sql_max_users, with_auth_timeout,
};
use async_trait::async_trait;
use pnpr_config::{MaxUsers, SqlBackendSettings};
use pnpr_error::{RegistryError, Result};
use sqlx::{MySqlConnection, MySqlPool, Row};

#[derive(Debug)]
pub(crate) struct MysqlDatabase {
    pool: std::sync::Arc<MySqlPool>,
}

mod accounts;
mod tokens;

pub(crate) type MysqlAuth = SqlAuth<MysqlDatabase>;

impl SqlAuth<MysqlDatabase> {
    pub(crate) async fn connect(
        settings: &SqlBackendSettings,
        max_users: MaxUsers,
    ) -> Result<Self> {
        let startup_options =
            pnpr_sql::mysql_options(settings, settings.startup_timeout, settings.startup_timeout)?;
        let startup_pool =
            with_auth_timeout(settings.startup_timeout, startup_options.connect(&settings.url))
                .await?;
        let startup_db = MysqlDatabase { pool: std::sync::Arc::new(startup_pool) };
        with_auth_timeout(settings.startup_timeout, startup_db.init_schema()).await?;
        startup_db.pool.close().await;

        let pool = pnpr_sql::mysql_pool(settings)?;
        let db = MysqlDatabase { pool };
        Ok(SqlAuth::new(db, max_users, settings.timeout))
    }
}

#[async_trait]
impl AuthSqlBackend for MysqlDatabase {
    async fn stored_user(&self, username: &str) -> Result<Option<StoredUser>> {
        let row = sqlx::query("SELECT username, bcrypt_hash, id FROM users WHERE username = ?")
            .bind(username)
            .fetch_optional(&*self.pool)
            .await?;
        row.map(|row| -> std::result::Result<StoredUser, sqlx::Error> {
            Ok(StoredUser {
                username: row.try_get(0)?,
                bcrypt_hash: row.try_get(1)?,
                id: row.try_get(2)?,
            })
        })
        .transpose()
        .map_err(RegistryError::from)
    }

    async fn account_name(&self, user_id: &str) -> Result<Option<String>> {
        Ok(sqlx::query_scalar("SELECT username FROM users WHERE id = ? AND bcrypt_hash <> '!'")
            .bind(user_id)
            .fetch_optional(&*self.pool)
            .await?)
    }

    async fn rename_user(&self, user_id: &str, username: &str) -> Result<()> {
        let current = self.account_name(user_id).await?.ok_or(RegistryError::NotFound)?;
        if current == username {
            return Ok(());
        }
        let result =
            sqlx::query("UPDATE users SET username = ?, username_skeleton_hash = ? WHERE id = ? AND bcrypt_hash <> '!'")
                .bind(username)
                .bind(crate::validation::username_skeleton_hash(username))
                .bind(user_id)
                .execute(&*self.pool)
                .await;
        match result {
            Ok(result) if result.rows_affected() > 0 => Ok(()),
            Ok(_) => Err(RegistryError::NotFound),
            Err(err) if is_unique_violation(&err) => Err(crate::validation::username_conflict(
                username,
                self.stored_user(username).await?.is_some(),
            )),
            Err(err) => Err(err.into()),
        }
    }

    async fn list_accounts(&self) -> Result<Vec<crate::UserAccount>> {
        let rows = sqlx::query(
            "SELECT id, username FROM users WHERE bcrypt_hash <> '!' ORDER BY username",
        )
        .fetch_all(&*self.pool)
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
        bcrypt_hash: &str,
        max_users: MaxUsers,
    ) -> Result<InsertUser> {
        let mut can_retry_after_reconcile = matches!(max_users, MaxUsers::Limited(_));
        let mut tx = loop {
            let mut tx = self.pool.begin().await?;
            if claim_user_slot(&mut tx, max_users).await? {
                break tx;
            }
            tx.rollback().await?;
            if !std::mem::take(&mut can_retry_after_reconcile)
                || !self.reconcile_user_counter_overcount_impl().await?
            {
                return self.existing_or_cap_reached(username).await;
            }
        };
        let id = crate::new_principal_id();
        let inserted = sqlx::query(
            "INSERT INTO users (username, bcrypt_hash, id, username_skeleton_hash) VALUES (?, ?, ?, ?)",
        )
        .bind(username)
        .bind(bcrypt_hash)
        .bind(&id)
        .bind(crate::validation::username_skeleton_hash(username))
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

    async fn list_usernames(&self) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT username FROM users WHERE bcrypt_hash <> '!' ORDER BY username",
        )
        .fetch_all(&*self.pool)
        .await?)
    }

    async fn update_password_hash(
        &self,
        user_id: &str,
        bcrypt_hash: &str,
    ) -> Result<PasswordChange> {
        let updated =
            sqlx::query("UPDATE users SET bcrypt_hash = ? WHERE id = ? AND bcrypt_hash <> '!'")
                .bind(bcrypt_hash)
                .bind(user_id)
                .execute(&*self.pool)
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
        let updated =
            sqlx::query("UPDATE users SET bcrypt_hash = ? WHERE id = ? AND bcrypt_hash = ?")
                .bind(new_hash)
                .bind(user_id)
                .bind(old_hash)
                .execute(&*self.pool)
                .await?;
        Ok(updated.rows_affected() > 0)
    }

    async fn delete_user(&self, user_id: &str) -> Result<UserRemoval> {
        self.remove_user_record(user_id).await
    }

    async fn check_readiness(&self) -> Result<()> {
        sqlx::query("SELECT 1;").execute(&*self.pool).await.map_err(RegistryError::from)?;
        Ok(())
    }
}

impl MysqlDatabase {
    async fn remove_user_record(&self, user_id: &str) -> Result<UserRemoval> {
        let mut tx = self.pool.begin().await?;
        let statement = "UPDATE users SET username = CONCAT('deleted:', id), username_skeleton_hash = NULL, bcrypt_hash = '!' WHERE id = ? AND bcrypt_hash <> '!'";
        let removed = sqlx::query(statement)
            .bind(user_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if removed > 0 {
            sqlx::query("UPDATE auth_counters SET value = value - 1 WHERE name = ? AND value > 0")
                .bind("users")
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(if removed > 0 { UserRemoval::Removed } else { UserRemoval::NoSuchUser })
    }

    async fn init_schema(&self) -> Result<()> {
        crate::schema::migrate_mysql(&self.pool).await?;
        self.ensure_user_counter().await
    }

    async fn ensure_user_counter(&self) -> Result<()> {
        let count = self.actual_user_count().await?;
        if self.user_counter().await?.is_some() {
            self.set_user_counter_floor(count).await?;
            return Ok(());
        }
        let inserted = sqlx::query("INSERT INTO auth_counters (name, value) VALUES (?, ?)")
            .bind("users")
            .bind(count)
            .execute(&*self.pool)
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
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE bcrypt_hash <> '!'")
            .fetch_one(&*self.pool)
            .await?;
        Ok(count.max(0))
    }

    async fn user_counter(&self) -> Result<Option<i64>> {
        let count: Option<i64> =
            sqlx::query_scalar("SELECT value FROM auth_counters WHERE name = ?")
                .bind("users")
                .fetch_optional(&*self.pool)
                .await?;
        Ok(count)
    }

    async fn set_user_counter_floor(&self, count: i64) -> Result<u64> {
        let updated = sqlx::query(
            "UPDATE auth_counters
             SET value = CASE WHEN value < ? THEN ? ELSE value END
             WHERE name = ?",
        )
        .bind(count)
        .bind(count)
        .bind("users")
        .execute(&*self.pool)
        .await?;
        Ok(updated.rows_affected())
    }

    async fn reconcile_user_counter_overcount_impl(&self) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let Some(counter): Option<i64> =
            sqlx::query_scalar("SELECT value FROM auth_counters WHERE name = ? FOR UPDATE")
                .bind("users")
                .fetch_optional(&mut *tx)
                .await?
        else {
            tx.commit().await?;
            return Ok(false);
        };
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE bcrypt_hash <> '!'")
            .fetch_one(&mut *tx)
            .await?;
        if counter <= count {
            tx.commit().await?;
            return Ok(false);
        }
        sqlx::query("UPDATE auth_counters SET value = ? WHERE name = ?")
            .bind(count.max(0))
            .bind("users")
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
        .is_some_and(|err| {
            err.code()
                .is_some_and(|code| code.as_ref() == "23000" || code.as_ref() == "1062")
        })
}

async fn claim_user_slot(connection: &mut MySqlConnection, max_users: MaxUsers) -> Result<bool> {
    match max_users {
        MaxUsers::Limited(max) => {
            let max = sql_max_users(max, "mysql")?;
            let updated = sqlx::query(
                "UPDATE auth_counters SET value = value + 1
                 WHERE name = ? AND value < ?",
            )
            .bind("users")
            .bind(max)
            .execute(connection)
            .await?;
            Ok(updated.rows_affected() != 0)
        }
        MaxUsers::Unlimited => {
            sqlx::query("UPDATE auth_counters SET value = value + 1 WHERE name = ?")
                .bind("users")
                .execute(connection)
                .await?;
            Ok(true)
        }
        MaxUsers::Disabled => Ok(true),
    }
}

#[cfg(test)]
mod on_mysql;
