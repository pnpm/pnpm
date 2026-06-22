//! Shared SQL auth backend for relational databases supported through
//! feature-gated `sqlx` drivers.

use super::{
    DEFAULT_BCRYPT_COST, TokenBackend, TokenRecord, UpsertOutcome, UserBackend, fresh_secret,
    hash_bcrypt, mint_token, sha256_hex, unix_seconds, validate_username, verify_bcrypt,
    verify_returning_user,
};
use crate::{
    config::MaxUsers,
    error::{RegistryError, Result},
};
use async_trait::async_trait;
use std::{
    future::Future,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

#[derive(Debug)]
pub(in crate::auth) struct SqlAuth<Db> {
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

/// Only use this around read-only request-path work or startup setup. Request-path
/// writes rely on the backend's statement timeout and must await the database
/// result so callers do not observe a timeout with an unknown commit state.
async fn with_auth_timeout<T, E>(
    timeout: Duration,
    future: impl Future<Output = std::result::Result<T, E>>,
) -> Result<T>
where
    RegistryError: From<E>,
{
    match tokio::time::timeout(timeout, future).await {
        Ok(result) => result.map_err(RegistryError::from),
        Err(_) => Err(RegistryError::AuthDatabaseTimeout),
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

        match self.max_users {
            MaxUsers::Disabled => return Err(RegistryError::RegistrationDisabled),
            MaxUsers::Limited(max) => {
                if with_auth_timeout(self.timeout, self.db.user_count()).await? >= max {
                    let reconciled_below_cap =
                        self.reconcile_capped_counter_once_per_interval().await?
                            && with_auth_timeout(self.timeout, self.db.user_count()).await? < max;
                    if !reconciled_below_cap {
                        return Err(RegistryError::TooManyUsers { max });
                    }
                }
            }
            MaxUsers::Unlimited => {}
        }

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

    async fn verify(&self, username: &str, password: &str) -> Result<Option<String>> {
        if validate_username(username).is_err() {
            return Ok(None);
        }

        let Some(stored) = with_auth_timeout(self.timeout, self.db.stored_user(username)).await?
        else {
            return Ok(None);
        };
        let valid = verify_bcrypt(password.to_string(), stored.bcrypt_hash).await?;
        Ok(valid.then_some(stored.username))
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

#[cfg(feature = "backend-postgres")]
pub(super) mod postgres {
    use super::super::{TokenRecord, token_timestamp_from_sql, token_timestamp_to_sql};
    use super::{
        AuthSqlBackend, InsertUser, SqlAuth, StoredUser, invalid_pool_size, sql_max_users,
        timeout_millis, with_auth_timeout,
    };
    use crate::{
        config::{MaxUsers, SqlBackendSettings},
        error::{RegistryError, Result},
    };
    use async_trait::async_trait;
    use sqlx::{PgPool, Row, postgres::PgPoolOptions};
    use std::time::Duration;

    #[derive(Debug)]
    pub(in crate::auth) struct PostgresDatabase {
        pool: PgPool,
    }

    pub(in crate::auth) type PostgresAuth = SqlAuth<PostgresDatabase>;

    impl SqlAuth<PostgresDatabase> {
        pub(in crate::auth) async fn connect(
            settings: &SqlBackendSettings,
            max_users: MaxUsers,
        ) -> Result<Self> {
            let startup_options = postgres_pool_options(
                settings,
                settings.startup_timeout,
                settings.startup_timeout,
            )?;
            let startup_pool =
                with_auth_timeout(settings.startup_timeout, startup_options.connect(&settings.url))
                    .await?;
            let startup_db = PostgresDatabase { pool: startup_pool };
            with_auth_timeout(settings.startup_timeout, startup_db.init_schema()).await?;
            startup_db.pool.close().await;

            let pool = postgres_pool_options(settings, settings.timeout, settings.timeout)?
                .connect_lazy(&settings.url)?;
            let db = PostgresDatabase { pool };
            Ok(SqlAuth::new(db, max_users, settings.timeout))
        }
    }

    fn postgres_pool_options(
        settings: &SqlBackendSettings,
        session_timeout: Duration,
        acquire_timeout: Duration,
    ) -> Result<PgPoolOptions> {
        let mut options = PgPoolOptions::new();
        if let Some(max_connections) = settings.max_connections {
            if max_connections == 0 {
                return Err(invalid_pool_size("postgres"));
            }
            options = options.max_connections(max_connections);
        }
        let statement_timeout_sql =
            format!("SET statement_timeout = {}", timeout_millis(session_timeout));
        options = options.after_connect(move |conn, _meta| {
            let statement_timeout_sql = statement_timeout_sql.clone();
            Box::pin(async move {
                sqlx::query(&statement_timeout_sql).execute(conn).await?;
                Ok(())
            })
        });
        Ok(options.acquire_timeout(acquire_timeout))
    }

    #[async_trait]
    impl AuthSqlBackend for PostgresDatabase {
        async fn stored_user(&self, username: &str) -> Result<Option<StoredUser>> {
            let row = sqlx::query("SELECT username, bcrypt_hash FROM users WHERE username = $1")
                .bind(username)
                .fetch_optional(&self.pool)
                .await?;
            row.map(|row| -> std::result::Result<StoredUser, sqlx::Error> {
                Ok(StoredUser { username: row.try_get(0)?, bcrypt_hash: row.try_get(1)? })
            })
            .transpose()
            .map_err(RegistryError::from)
        }

        async fn user_count(&self) -> Result<u64> {
            let Some(count) = self.user_counter().await? else {
                self.ensure_user_counter().await?;
                return Ok(self.user_counter().await?.unwrap_or(0).max(0) as u64);
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
            loop {
                let mut tx = self.pool.begin().await?;
                match max_users {
                    MaxUsers::Limited(max) => {
                        let max = sql_max_users(max, "postgres")?;
                        let updated = sqlx::query(
                            "UPDATE auth_counters SET value = value + 1
                             WHERE name = $1 AND value < $2",
                        )
                        .bind("users")
                        .bind(max)
                        .execute(&mut *tx)
                        .await?;
                        if updated.rows_affected() == 0 {
                            tx.rollback().await?;
                            if can_retry_after_reconcile {
                                can_retry_after_reconcile = false;
                                if self.reconcile_user_counter_overcount_impl().await? {
                                    continue;
                                }
                            }
                            return self.existing_or_cap_reached(username).await;
                        }
                    }
                    MaxUsers::Unlimited => {
                        sqlx::query("UPDATE auth_counters SET value = value + 1 WHERE name = $1")
                            .bind("users")
                            .execute(&mut *tx)
                            .await?;
                    }
                    MaxUsers::Disabled => {}
                }
                let inserted =
                    sqlx::query("INSERT INTO users (username, bcrypt_hash) VALUES ($1, $2)")
                        .bind(username)
                        .bind(bcrypt_hash)
                        .execute(&mut *tx)
                        .await;
                match inserted {
                    Ok(_) => {
                        tx.commit().await?;
                        return Ok(InsertUser::Created);
                    }
                    Err(err) if is_unique_violation(&err) => {
                        tx.rollback().await?;
                        return self.existing_or_cap_reached(username).await;
                    }
                    Err(err) => return Err(err.into()),
                }
            }
        }

        async fn insert_token(&self, token_hash: &str, record: &TokenRecord) -> Result<()> {
            let cidr_json = serde_json::to_string(&record.cidr_whitelist)
                .expect("Vec<String> always serializes to JSON");
            sqlx::query(
                "INSERT INTO tokens
                    (token_hash, username, created_at, last_used_at, readonly, cidr_whitelist)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(token_hash)
            .bind(&record.username)
            .bind(token_timestamp_to_sql(record.created_at))
            .bind(token_timestamp_to_sql(record.last_used_at))
            .bind(i16::from(record.readonly))
            .bind(cidr_json)
            .execute(&self.pool)
            .await?;
            Ok(())
        }

        async fn lookup_token(&self, token_hash: &str) -> Result<Option<String>> {
            let row = sqlx::query("SELECT username FROM tokens WHERE token_hash = $1")
                .bind(token_hash)
                .fetch_optional(&self.pool)
                .await?;
            row.map(|row| row.try_get(0)).transpose().map_err(RegistryError::from)
        }

        async fn find_token(&self, token_hash: &str) -> Result<Option<TokenRecord>> {
            let row = sqlx::query(
                "SELECT username, created_at, last_used_at, readonly, cidr_whitelist
                 FROM tokens WHERE token_hash = $1",
            )
            .bind(token_hash)
            .fetch_optional(&self.pool)
            .await?;
            row.map(|row| token_record_from_row(&row)).transpose()
        }

        async fn list_tokens(&self, username: &str) -> Result<Vec<(String, TokenRecord)>> {
            let rows = sqlx::query(
                "SELECT token_hash, username, created_at, last_used_at, readonly, cidr_whitelist
                 FROM tokens WHERE username = $1",
            )
            .bind(username)
            .fetch_all(&self.pool)
            .await?;
            rows.into_iter().map(|row| keyed_token_record_from_row(&row)).collect()
        }

        async fn delete_token(&self, token_hash: &str) -> Result<()> {
            sqlx::query("DELETE FROM tokens WHERE token_hash = $1")
                .bind(token_hash)
                .execute(&self.pool)
                .await?;
            Ok(())
        }
    }

    impl PostgresDatabase {
        async fn init_schema(&self) -> Result<()> {
            sqlx::query(super::super::USERS_TABLE_SQL).execute(&self.pool).await?;
            sqlx::query(super::super::TOKENS_TABLE_SQL).execute(&self.pool).await?;
            sqlx::query(super::super::TOKENS_INDEX_SQL).execute(&self.pool).await?;
            sqlx::query(super::super::AUTH_COUNTERS_TABLE_SQL).execute(&self.pool).await?;
            self.ensure_user_counter().await
        }

        async fn ensure_user_counter(&self) -> Result<()> {
            let count = self.actual_user_count().await?;
            if self.set_user_counter_floor(count).await? > 0 {
                return Ok(());
            }
            let inserted = sqlx::query("INSERT INTO auth_counters (name, value) VALUES ($1, $2)")
                .bind("users")
                .bind(count)
                .execute(&self.pool)
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
            let count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&self.pool).await?;
            Ok(count.max(0))
        }

        async fn user_counter(&self) -> Result<Option<i64>> {
            let count: Option<i64> =
                sqlx::query_scalar("SELECT value FROM auth_counters WHERE name = $1")
                    .bind("users")
                    .fetch_optional(&self.pool)
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
            .execute(&self.pool)
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
            let count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&mut *tx).await?;
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

    fn keyed_token_record_from_row(row: &sqlx::postgres::PgRow) -> Result<(String, TokenRecord)> {
        Ok((row.try_get(0)?, token_record_from_offset(row, 1)?))
    }

    fn token_record_from_row(row: &sqlx::postgres::PgRow) -> Result<TokenRecord> {
        token_record_from_offset(row, 0)
    }

    fn token_record_from_offset(row: &sqlx::postgres::PgRow, offset: usize) -> Result<TokenRecord> {
        let cidr_json: String = row.try_get(offset + 4)?;
        let cidr_whitelist: Vec<String> = serde_json::from_str(&cidr_json).unwrap_or_default();
        let readonly: i16 = row.try_get(offset + 3)?;
        Ok(TokenRecord {
            username: row.try_get(offset)?,
            created_at: token_timestamp_from_sql(row.try_get(offset + 1)?),
            last_used_at: token_timestamp_from_sql(row.try_get(offset + 2)?),
            readonly: readonly != 0,
            cidr_whitelist,
        })
    }

    fn is_unique_violation(err: &sqlx::Error) -> bool {
        err.as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .is_some_and(|code| code.as_ref() == "23505")
    }
}

#[cfg(feature = "backend-mysql")]
pub(super) mod mysql {
    use super::super::{TokenRecord, token_timestamp_from_sql, token_timestamp_to_sql};
    use super::{
        AuthSqlBackend, InsertUser, SqlAuth, StoredUser, invalid_pool_size, sql_max_users,
        timeout_millis, timeout_seconds, with_auth_timeout,
    };
    use crate::{
        config::{MaxUsers, SqlBackendSettings},
        error::{RegistryError, Result},
    };
    use async_trait::async_trait;
    use sqlx::{MySqlPool, Row, mysql::MySqlPoolOptions};
    use std::time::Duration;

    #[derive(Debug)]
    pub(in crate::auth) struct MysqlDatabase {
        pool: MySqlPool,
    }

    pub(in crate::auth) type MysqlAuth = SqlAuth<MysqlDatabase>;

    impl SqlAuth<MysqlDatabase> {
        pub(in crate::auth) async fn connect(
            settings: &SqlBackendSettings,
            max_users: MaxUsers,
        ) -> Result<Self> {
            let startup_options =
                mysql_pool_options(settings, settings.startup_timeout, settings.startup_timeout)?;
            let startup_pool =
                with_auth_timeout(settings.startup_timeout, startup_options.connect(&settings.url))
                    .await?;
            let startup_db = MysqlDatabase { pool: startup_pool };
            with_auth_timeout(settings.startup_timeout, startup_db.init_schema()).await?;
            startup_db.pool.close().await;

            let pool = mysql_pool_options(settings, settings.timeout, settings.timeout)?
                .connect_lazy(&settings.url)?;
            let db = MysqlDatabase { pool };
            Ok(SqlAuth::new(db, max_users, settings.timeout))
        }
    }

    fn mysql_pool_options(
        settings: &SqlBackendSettings,
        session_timeout: Duration,
        acquire_timeout: Duration,
    ) -> Result<MySqlPoolOptions> {
        let mut options = MySqlPoolOptions::new();
        if let Some(max_connections) = settings.max_connections {
            if max_connections == 0 {
                return Err(invalid_pool_size("mysql"));
            }
            options = options.max_connections(max_connections);
        }
        let statement_timeout_sql =
            format!("SET SESSION max_execution_time = {}", timeout_millis(session_timeout));
        let row_lock_timeout_sql =
            format!("SET SESSION innodb_lock_wait_timeout = {}", timeout_seconds(session_timeout));
        let metadata_lock_timeout_sql =
            format!("SET SESSION lock_wait_timeout = {}", timeout_seconds(session_timeout));
        options = options.after_connect(move |conn, _meta| {
            let statement_timeout_sql = statement_timeout_sql.clone();
            let row_lock_timeout_sql = row_lock_timeout_sql.clone();
            let metadata_lock_timeout_sql = metadata_lock_timeout_sql.clone();
            Box::pin(async move {
                sqlx::query(&statement_timeout_sql).execute(&mut *conn).await?;
                sqlx::query(&row_lock_timeout_sql).execute(&mut *conn).await?;
                sqlx::query(&metadata_lock_timeout_sql).execute(conn).await?;
                Ok(())
            })
        });
        Ok(options.acquire_timeout(acquire_timeout))
    }

    #[async_trait]
    impl AuthSqlBackend for MysqlDatabase {
        async fn stored_user(&self, username: &str) -> Result<Option<StoredUser>> {
            let row = sqlx::query("SELECT username, bcrypt_hash FROM users WHERE username = ?")
                .bind(username)
                .fetch_optional(&self.pool)
                .await?;
            row.map(|row| -> std::result::Result<StoredUser, sqlx::Error> {
                Ok(StoredUser { username: row.try_get(0)?, bcrypt_hash: row.try_get(1)? })
            })
            .transpose()
            .map_err(RegistryError::from)
        }

        async fn user_count(&self) -> Result<u64> {
            let Some(count) = self.user_counter().await? else {
                self.ensure_user_counter().await?;
                return Ok(self.user_counter().await?.unwrap_or(0).max(0) as u64);
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
            loop {
                let mut tx = self.pool.begin().await?;
                match max_users {
                    MaxUsers::Limited(max) => {
                        let max = sql_max_users(max, "mysql")?;
                        let updated = sqlx::query(
                            "UPDATE auth_counters SET value = value + 1
                             WHERE name = ? AND value < ?",
                        )
                        .bind("users")
                        .bind(max)
                        .execute(&mut *tx)
                        .await?;
                        if updated.rows_affected() == 0 {
                            tx.rollback().await?;
                            if can_retry_after_reconcile {
                                can_retry_after_reconcile = false;
                                if self.reconcile_user_counter_overcount_impl().await? {
                                    continue;
                                }
                            }
                            return self.existing_or_cap_reached(username).await;
                        }
                    }
                    MaxUsers::Unlimited => {
                        sqlx::query("UPDATE auth_counters SET value = value + 1 WHERE name = ?")
                            .bind("users")
                            .execute(&mut *tx)
                            .await?;
                    }
                    MaxUsers::Disabled => {}
                }
                let inserted =
                    sqlx::query("INSERT INTO users (username, bcrypt_hash) VALUES (?, ?)")
                        .bind(username)
                        .bind(bcrypt_hash)
                        .execute(&mut *tx)
                        .await;
                match inserted {
                    Ok(_) => {
                        tx.commit().await?;
                        return Ok(InsertUser::Created);
                    }
                    Err(err) if is_unique_violation(&err) => {
                        tx.rollback().await?;
                        return self.existing_or_cap_reached(username).await;
                    }
                    Err(err) => return Err(err.into()),
                }
            }
        }

        async fn insert_token(&self, token_hash: &str, record: &TokenRecord) -> Result<()> {
            let cidr_json = serde_json::to_string(&record.cidr_whitelist)
                .expect("Vec<String> always serializes to JSON");
            sqlx::query(
                "INSERT INTO tokens
                    (token_hash, username, created_at, last_used_at, readonly, cidr_whitelist)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(token_hash)
            .bind(&record.username)
            .bind(token_timestamp_to_sql(record.created_at))
            .bind(token_timestamp_to_sql(record.last_used_at))
            .bind(i16::from(record.readonly))
            .bind(cidr_json)
            .execute(&self.pool)
            .await?;
            Ok(())
        }

        async fn lookup_token(&self, token_hash: &str) -> Result<Option<String>> {
            let row = sqlx::query("SELECT username FROM tokens WHERE token_hash = ?")
                .bind(token_hash)
                .fetch_optional(&self.pool)
                .await?;
            row.map(|row| row.try_get(0)).transpose().map_err(RegistryError::from)
        }

        async fn find_token(&self, token_hash: &str) -> Result<Option<TokenRecord>> {
            let row = sqlx::query(
                "SELECT username, created_at, last_used_at, readonly, cidr_whitelist
                 FROM tokens WHERE token_hash = ?",
            )
            .bind(token_hash)
            .fetch_optional(&self.pool)
            .await?;
            row.map(|row| token_record_from_row(&row)).transpose()
        }

        async fn list_tokens(&self, username: &str) -> Result<Vec<(String, TokenRecord)>> {
            let rows = sqlx::query(
                "SELECT token_hash, username, created_at, last_used_at, readonly, cidr_whitelist
                 FROM tokens WHERE username = ?",
            )
            .bind(username)
            .fetch_all(&self.pool)
            .await?;
            rows.into_iter().map(|row| keyed_token_record_from_row(&row)).collect()
        }

        async fn delete_token(&self, token_hash: &str) -> Result<()> {
            sqlx::query("DELETE FROM tokens WHERE token_hash = ?")
                .bind(token_hash)
                .execute(&self.pool)
                .await?;
            Ok(())
        }
    }

    impl MysqlDatabase {
        async fn init_schema(&self) -> Result<()> {
            sqlx::query(super::super::USERS_TABLE_SQL).execute(&self.pool).await?;
            sqlx::query(super::super::TOKENS_TABLE_SQL).execute(&self.pool).await?;
            create_token_index(&self.pool).await?;
            sqlx::query(super::super::AUTH_COUNTERS_TABLE_SQL).execute(&self.pool).await?;
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
                .execute(&self.pool)
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
            let count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&self.pool).await?;
            Ok(count.max(0))
        }

        async fn user_counter(&self) -> Result<Option<i64>> {
            let count: Option<i64> =
                sqlx::query_scalar("SELECT value FROM auth_counters WHERE name = ?")
                    .bind("users")
                    .fetch_optional(&self.pool)
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
            .execute(&self.pool)
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
            let count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&mut *tx).await?;
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

    async fn create_token_index(pool: &MySqlPool) -> Result<()> {
        let result =
            sqlx::query("CREATE INDEX tokens_username ON tokens(username)").execute(pool).await;
        match result {
            Ok(_) => Ok(()),
            Err(err) if is_duplicate_index(&err) => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    fn keyed_token_record_from_row(row: &sqlx::mysql::MySqlRow) -> Result<(String, TokenRecord)> {
        Ok((row.try_get(0)?, token_record_from_offset(row, 1)?))
    }

    fn token_record_from_row(row: &sqlx::mysql::MySqlRow) -> Result<TokenRecord> {
        token_record_from_offset(row, 0)
    }

    fn token_record_from_offset(row: &sqlx::mysql::MySqlRow, offset: usize) -> Result<TokenRecord> {
        let cidr_json: String = row.try_get(offset + 4)?;
        let cidr_whitelist: Vec<String> = serde_json::from_str(&cidr_json).unwrap_or_default();
        let readonly: i16 = row.try_get(offset + 3)?;
        Ok(TokenRecord {
            username: row.try_get(offset)?,
            created_at: token_timestamp_from_sql(row.try_get(offset + 1)?),
            last_used_at: token_timestamp_from_sql(row.try_get(offset + 2)?),
            readonly: readonly != 0,
            cidr_whitelist,
        })
    }

    fn is_unique_violation(err: &sqlx::Error) -> bool {
        err.as_database_error().is_some_and(|err| {
            err.code().is_some_and(|code| code.as_ref() == "23000" || code.as_ref() == "1062")
        })
    }

    fn is_duplicate_index(err: &sqlx::Error) -> bool {
        err.as_database_error()
            .and_then(|err| err.try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>())
            .is_some_and(|err| err.number() == 1061)
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
mod tests {
    use super::super::MAX_USERNAME_CHARS;
    use super::*;
    use std::sync::Arc;

    struct CanonicalBackend {
        user: StoredUser,
    }

    struct SlowLookupBackend;

    struct SlowWriteBackend;

    struct CountingLookupBackend {
        stored_user_calls: Arc<AtomicU64>,
    }

    struct CappedBackend {
        reconcile_calls: Arc<AtomicU64>,
    }

    #[async_trait]
    impl AuthSqlBackend for CanonicalBackend {
        async fn stored_user(&self, _username: &str) -> Result<Option<StoredUser>> {
            Ok(Some(self.user.clone()))
        }

        async fn user_count(&self) -> Result<u64> {
            Ok(1)
        }

        async fn reconcile_user_counter_overcount(&self) -> Result<bool> {
            Ok(false)
        }

        async fn insert_user(
            &self,
            _username: &str,
            _bcrypt_hash: &str,
            _max_users: MaxUsers,
        ) -> Result<InsertUser> {
            Ok(InsertUser::Existing(self.user.clone()))
        }

        async fn insert_token(&self, _token_hash: &str, _record: &TokenRecord) -> Result<()> {
            Ok(())
        }

        async fn lookup_token(&self, _token_hash: &str) -> Result<Option<String>> {
            Ok(None)
        }

        async fn find_token(&self, _token_hash: &str) -> Result<Option<TokenRecord>> {
            Ok(None)
        }

        async fn list_tokens(&self, _username: &str) -> Result<Vec<(String, TokenRecord)>> {
            Ok(Vec::new())
        }

        async fn delete_token(&self, _token_hash: &str) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl AuthSqlBackend for SlowLookupBackend {
        async fn stored_user(&self, _username: &str) -> Result<Option<StoredUser>> {
            Ok(None)
        }

        async fn user_count(&self) -> Result<u64> {
            Ok(0)
        }

        async fn reconcile_user_counter_overcount(&self) -> Result<bool> {
            Ok(false)
        }

        async fn insert_user(
            &self,
            _username: &str,
            _bcrypt_hash: &str,
            _max_users: MaxUsers,
        ) -> Result<InsertUser> {
            Ok(InsertUser::CapReached)
        }

        async fn insert_token(&self, _token_hash: &str, _record: &TokenRecord) -> Result<()> {
            Ok(())
        }

        async fn lookup_token(&self, _token_hash: &str) -> Result<Option<String>> {
            tokio::time::sleep(Duration::from_mins(1)).await;
            Ok(None)
        }

        async fn find_token(&self, _token_hash: &str) -> Result<Option<TokenRecord>> {
            Ok(None)
        }

        async fn list_tokens(&self, _username: &str) -> Result<Vec<(String, TokenRecord)>> {
            Ok(Vec::new())
        }

        async fn delete_token(&self, _token_hash: &str) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl AuthSqlBackend for SlowWriteBackend {
        async fn stored_user(&self, _username: &str) -> Result<Option<StoredUser>> {
            Ok(None)
        }

        async fn user_count(&self) -> Result<u64> {
            Ok(0)
        }

        async fn reconcile_user_counter_overcount(&self) -> Result<bool> {
            Ok(false)
        }

        async fn insert_user(
            &self,
            _username: &str,
            _bcrypt_hash: &str,
            _max_users: MaxUsers,
        ) -> Result<InsertUser> {
            Ok(InsertUser::Created)
        }

        async fn insert_token(&self, _token_hash: &str, _record: &TokenRecord) -> Result<()> {
            tokio::time::sleep(Duration::from_millis(20)).await;
            Ok(())
        }

        async fn lookup_token(&self, _token_hash: &str) -> Result<Option<String>> {
            Ok(None)
        }

        async fn find_token(&self, _token_hash: &str) -> Result<Option<TokenRecord>> {
            Ok(None)
        }

        async fn list_tokens(&self, _username: &str) -> Result<Vec<(String, TokenRecord)>> {
            Ok(Vec::new())
        }

        async fn delete_token(&self, _token_hash: &str) -> Result<()> {
            tokio::time::sleep(Duration::from_millis(20)).await;
            Ok(())
        }
    }

    #[async_trait]
    impl AuthSqlBackend for CountingLookupBackend {
        async fn stored_user(&self, _username: &str) -> Result<Option<StoredUser>> {
            self.stored_user_calls.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }

        async fn user_count(&self) -> Result<u64> {
            Ok(0)
        }

        async fn reconcile_user_counter_overcount(&self) -> Result<bool> {
            Ok(false)
        }

        async fn insert_user(
            &self,
            _username: &str,
            _bcrypt_hash: &str,
            _max_users: MaxUsers,
        ) -> Result<InsertUser> {
            Ok(InsertUser::Created)
        }

        async fn insert_token(&self, _token_hash: &str, _record: &TokenRecord) -> Result<()> {
            Ok(())
        }

        async fn lookup_token(&self, _token_hash: &str) -> Result<Option<String>> {
            Ok(None)
        }

        async fn find_token(&self, _token_hash: &str) -> Result<Option<TokenRecord>> {
            Ok(None)
        }

        async fn list_tokens(&self, _username: &str) -> Result<Vec<(String, TokenRecord)>> {
            Ok(Vec::new())
        }

        async fn delete_token(&self, _token_hash: &str) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl AuthSqlBackend for CappedBackend {
        async fn stored_user(&self, _username: &str) -> Result<Option<StoredUser>> {
            Ok(None)
        }

        async fn user_count(&self) -> Result<u64> {
            Ok(1)
        }

        async fn reconcile_user_counter_overcount(&self) -> Result<bool> {
            self.reconcile_calls.fetch_add(1, Ordering::SeqCst);
            Ok(false)
        }

        async fn insert_user(
            &self,
            _username: &str,
            _bcrypt_hash: &str,
            _max_users: MaxUsers,
        ) -> Result<InsertUser> {
            panic!("capped precheck should reject before insert_user")
        }

        async fn insert_token(&self, _token_hash: &str, _record: &TokenRecord) -> Result<()> {
            Ok(())
        }

        async fn lookup_token(&self, _token_hash: &str) -> Result<Option<String>> {
            Ok(None)
        }

        async fn find_token(&self, _token_hash: &str) -> Result<Option<TokenRecord>> {
            Ok(None)
        }

        async fn list_tokens(&self, _username: &str) -> Result<Vec<(String, TokenRecord)>> {
            Ok(Vec::new())
        }

        async fn delete_token(&self, _token_hash: &str) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn verify_returns_the_stored_username() {
        let bcrypt_hash = bcrypt::hash("secret", 4).unwrap();
        let auth = SqlAuth::new(
            CanonicalBackend { user: StoredUser { username: "Alice".to_string(), bcrypt_hash } },
            MaxUsers::Unlimited,
            Duration::from_secs(30),
        );

        assert_eq!(auth.verify("alice", "secret").await.unwrap().as_deref(), Some("Alice"));
    }

    #[tokio::test]
    async fn verify_propagates_corrupt_hash_errors() {
        let auth = SqlAuth::new(
            CanonicalBackend {
                user: StoredUser {
                    username: "Alice".to_string(),
                    bcrypt_hash: "not-a-bcrypt-hash".to_string(),
                },
            },
            MaxUsers::Unlimited,
            Duration::from_secs(30),
        );

        let err = auth.verify("alice", "secret").await.unwrap_err();

        assert!(matches!(err, RegistryError::Bcrypt(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn verify_skips_unbounded_usernames_without_db_lookup() {
        let stored_user_calls = Arc::new(AtomicU64::new(0));
        let auth = SqlAuth::new(
            CountingLookupBackend { stored_user_calls: Arc::clone(&stored_user_calls) },
            MaxUsers::Unlimited,
            Duration::from_secs(30),
        );
        let overlong = "a".repeat(MAX_USERNAME_CHARS + 1);

        for username in ["", " alice", "alice ", "#alice", "alice:admin", "alice\nadmin"] {
            assert_eq!(auth.verify(username, "secret").await.unwrap(), None);
        }
        assert_eq!(auth.verify(&overlong, "secret").await.unwrap(), None);
        assert_eq!(stored_user_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn add_or_login_returns_the_stored_username_for_existing_users() {
        let bcrypt_hash = bcrypt::hash("secret", 4).unwrap();
        let auth = SqlAuth::new(
            CanonicalBackend { user: StoredUser { username: "Alice".to_string(), bcrypt_hash } },
            MaxUsers::Unlimited,
            Duration::from_secs(30),
        );

        let outcome = auth.add_or_login("alice", "secret").await.unwrap();

        assert!(matches!(outcome, (UpsertOutcome::LoggedIn, _)));
        assert_eq!(outcome.1, "Alice");
    }

    #[tokio::test]
    async fn add_or_login_rejects_invalid_usernames_without_db_lookup() {
        let stored_user_calls = Arc::new(AtomicU64::new(0));
        let auth = SqlAuth::new(
            CountingLookupBackend { stored_user_calls: Arc::clone(&stored_user_calls) },
            MaxUsers::Unlimited,
            Duration::from_secs(30),
        );
        let overlong = "a".repeat(MAX_USERNAME_CHARS + 1);

        for username in ["", " alice", "alice ", "#alice", "alice:admin", "alice\nadmin"] {
            let err = auth.add_or_login(username, "secret").await.unwrap_err();
            assert_eq!(
                err.status_code(),
                axum::http::StatusCode::BAD_REQUEST,
                "expected {username:?} to be rejected",
            );
        }
        let err = auth.add_or_login(&overlong, "secret").await.unwrap_err();
        assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(stored_user_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn add_or_login_rate_limits_capped_reconciliation() {
        let reconcile_calls = Arc::new(AtomicU64::new(0));
        let auth = SqlAuth::new(
            CappedBackend { reconcile_calls: Arc::clone(&reconcile_calls) },
            MaxUsers::Limited(1),
            Duration::from_secs(30),
        );

        for username in ["alice", "bob", "carol"] {
            let err = auth.add_or_login(username, "secret").await.unwrap_err();
            assert!(matches!(err, RegistryError::TooManyUsers { max: 1 }));
        }

        assert_eq!(reconcile_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn token_lookup_times_out_when_the_backend_stalls() {
        let auth = SqlAuth::new(SlowLookupBackend, MaxUsers::Unlimited, Duration::from_millis(1));

        let err = auth.lookup("token").await.unwrap_err();

        assert!(matches!(err, RegistryError::AuthDatabaseTimeout));
    }

    #[tokio::test]
    async fn token_issue_waits_for_slow_backend_write() {
        let auth = SqlAuth::new(SlowWriteBackend, MaxUsers::Unlimited, Duration::from_millis(1));

        let token = auth.issue("alice").await.unwrap();

        assert!(!token.is_empty());
    }
}
