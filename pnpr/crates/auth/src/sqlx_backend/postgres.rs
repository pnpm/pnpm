use super::{
    super::{TokenRecord, token_timestamp_from_sql, token_timestamp_to_sql},
    AuthSqlBackend, InsertUser, SqlAuth, StoredUser, invalid_pool_size, sql_max_users,
    timeout_millis, with_auth_timeout,
};
use async_trait::async_trait;
use pnpr_config::{MaxUsers, SqlBackendSettings};
use pnpr_error::{RegistryError, Result};
use sqlx::{PgConnection, PgPool, Row, postgres::PgPoolOptions};
use std::time::Duration;

#[derive(Debug)]
pub(crate) struct PostgresDatabase {
    pool: PgPool,
}

pub(crate) type PostgresAuth = SqlAuth<PostgresDatabase>;

impl SqlAuth<PostgresDatabase> {
    pub(crate) async fn connect(
        settings: &SqlBackendSettings,
        max_users: MaxUsers,
    ) -> Result<Self> {
        let startup_options =
            postgres_pool_options(settings, settings.startup_timeout, settings.startup_timeout)?;
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
        let inserted = sqlx::query("INSERT INTO users (username, bcrypt_hash) VALUES ($1, $2)")
            .bind(username)
            .bind(bcrypt_hash)
            .execute(&mut *tx)
            .await;
        match inserted {
            Ok(_) => {
                tx.commit().await?;
                Ok(InsertUser::Created)
            }
            Err(err) if is_unique_violation(&err) => {
                tx.rollback().await?;
                self.existing_or_cap_reached(username).await
            }
            Err(err) => Err(err.into()),
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
        row.map(|row| token_record_from_row(&row, token_hash)).transpose()
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
        sqlx::query(super::super::token_store::TOKENS_TABLE_SQL).execute(&self.pool).await?;
        sqlx::query(super::super::token_store::TOKENS_INDEX_SQL).execute(&self.pool).await?;
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
    let token_hash: String = row.try_get(0)?;
    let record = token_record_from_offset(row, 1, &token_hash)?;
    Ok((token_hash, record))
}

fn token_record_from_row(row: &sqlx::postgres::PgRow, token_hash: &str) -> Result<TokenRecord> {
    token_record_from_offset(row, 0, token_hash)
}

fn token_record_from_offset(
    row: &sqlx::postgres::PgRow,
    offset: usize,
    token_hash: &str,
) -> Result<TokenRecord> {
    let cidr_json: String = row.try_get(offset + 4)?;
    let cidr_whitelist: Vec<String> =
        serde_json::from_str(&cidr_json).map_err(|err| RegistryError::Internal {
            reason: format!("token {token_hash} has an unreadable cidr_whitelist: {err}"),
        })?;
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
