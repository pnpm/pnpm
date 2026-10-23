use crate::token_store::unix_seconds;
use pnpr_error::{RegistryError, Result};
use rusqlite::OptionalExtension as _;
use std::future::Future;

/// Current version of the auth database schema.
pub const CURRENT_SCHEMA_VERSION: i64 = 2;

/// Table storing schema migrations applied to the auth database.
pub(crate) const SCHEMA_MIGRATIONS_TABLE_SQL: &str =
    "CREATE TABLE IF NOT EXISTS auth_schema_migrations (
        version     BIGINT PRIMARY KEY,
        applied_at  BIGINT NOT NULL
    )";

fn check_schema_version(max_version: Option<i64>) -> Result<()> {
    if let Some(version) = max_version
        && version > CURRENT_SCHEMA_VERSION
    {
        return Err(RegistryError::Internal {
            reason: format!(
                "auth database schema version {version} is newer than supported version {CURRENT_SCHEMA_VERSION}; please upgrade pnpr",
            ),
        });
    }
    Ok(())
}

/// Run schema migrations on a `SQLite` token database.
pub(crate) fn migrate_sqlite(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute(SCHEMA_MIGRATIONS_TABLE_SQL, [])?;
    let max_version: Option<i64> = conn
        .query_row("SELECT MAX(version) FROM auth_schema_migrations", [], |row| row.get(0))
        .optional()?
        .flatten();
    check_schema_version(max_version)?;
    let current = max_version.unwrap_or(0);
    if current < 1 {
        apply_sqlite_v1(conn)?;
    }
    if current < 2 {
        apply_sqlite_v2(conn)?;
    }
    Ok(())
}

fn apply_sqlite_v1(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute(super::token_store::sqlite::TOKENS_TABLE_SQL, [])?;
    conn.execute(super::token_store::sqlite::TOKENS_INDEX_SQL, [])?;
    let now = unix_seconds() as i64;
    conn.execute(
        "INSERT INTO auth_schema_migrations (version, applied_at) VALUES (1, ?1) ON CONFLICT (version) DO NOTHING",
        rusqlite::params![now],
    )?;
    Ok(())
}

fn apply_sqlite_v2(conn: &rusqlite::Connection) -> Result<()> {
    add_column_if_missing_sqlite(conn, "ALTER TABLE tokens ADD COLUMN name VARCHAR(255)")?;
    add_column_if_missing_sqlite(conn, "ALTER TABLE tokens ADD COLUMN expires_at BIGINT")?;
    add_column_if_missing_sqlite(conn, "ALTER TABLE tokens ADD COLUMN scopes TEXT")?;
    add_column_if_missing_sqlite(conn, "ALTER TABLE tokens ADD COLUMN kind VARCHAR(64)")?;
    let now = unix_seconds() as i64;
    conn.execute(
        "INSERT INTO auth_schema_migrations (version, applied_at) VALUES (2, ?1) ON CONFLICT (version) DO NOTHING",
        rusqlite::params![now],
    )?;
    Ok(())
}

fn add_column_if_missing_sqlite(conn: &rusqlite::Connection, sql: &str) -> Result<()> {
    match conn.execute(sql, []) {
        Ok(_) => Ok(()),
        Err(err) if err.to_string().contains("duplicate column name") => Ok(()),
        Err(err) => Err(err.into()),
    }
}

/// Run schema migrations on a libsql database. `sync` runs after each
/// batch of writes, before a read that depends on them, for a connection
/// whose writes land elsewhere than its reads, such as an embedded replica.
#[cfg(feature = "backend-libsql")]
pub(crate) async fn migrate_libsql<SyncFuture>(
    conn: &libsql::Connection,
    sync: impl Fn() -> SyncFuture + Send + Sync,
) -> Result<()>
where
    SyncFuture: Future<Output = Result<()>> + Send,
{
    conn.execute(SCHEMA_MIGRATIONS_TABLE_SQL, ()).await?;
    sync().await?;
    let mut rows = conn.query("SELECT MAX(version) FROM auth_schema_migrations", ()).await?;
    let max_version: Option<i64> =
        if let Some(row) = rows.next().await? { row.get(0)? } else { None };
    check_schema_version(max_version)?;
    let current = max_version.unwrap_or(0);
    if current < 1 {
        apply_libsql_v1(conn).await?;
    }
    if current < 2 {
        apply_libsql_v2(conn).await?;
    }
    sync().await
}

#[cfg(feature = "backend-libsql")]
async fn apply_libsql_v1(conn: &libsql::Connection) -> Result<()> {
    conn.execute(super::USERS_TABLE_SQL, ()).await?;
    conn.execute(super::token_store::sqlite::TOKENS_TABLE_SQL, ()).await?;
    conn.execute(super::token_store::sqlite::TOKENS_INDEX_SQL, ()).await?;
    conn.execute(super::AUTH_COUNTERS_TABLE_SQL, ()).await?;
    let now = unix_seconds() as i64;
    conn.execute(
        "INSERT INTO auth_schema_migrations (version, applied_at) VALUES (1, ?1) ON CONFLICT (version) DO NOTHING",
        libsql::params![now],
    )
    .await?;
    Ok(())
}

#[cfg(feature = "backend-libsql")]
async fn apply_libsql_v2(conn: &libsql::Connection) -> Result<()> {
    add_column_if_missing_libsql(conn, "ALTER TABLE tokens ADD COLUMN name VARCHAR(255)").await?;
    add_column_if_missing_libsql(conn, "ALTER TABLE tokens ADD COLUMN expires_at BIGINT").await?;
    add_column_if_missing_libsql(conn, "ALTER TABLE tokens ADD COLUMN scopes TEXT").await?;
    add_column_if_missing_libsql(conn, "ALTER TABLE tokens ADD COLUMN kind VARCHAR(64)").await?;
    let now = unix_seconds() as i64;
    conn.execute(
        "INSERT INTO auth_schema_migrations (version, applied_at) VALUES (2, ?1) ON CONFLICT (version) DO NOTHING",
        libsql::params![now],
    )
    .await?;
    Ok(())
}

#[cfg(feature = "backend-libsql")]
async fn add_column_if_missing_libsql(conn: &libsql::Connection, sql: &str) -> Result<()> {
    match conn.execute(sql, ()).await {
        Ok(_) => Ok(()),
        Err(err) if err.to_string().contains("duplicate column name") => Ok(()),
        Err(err) => Err(err.into()),
    }
}

/// Run schema migrations on a `PostgreSQL` database.
#[cfg(feature = "backend-postgres")]
pub(crate) async fn migrate_postgres(pool: &sqlx::PgPool) -> Result<()> {
    sqlx::query(SCHEMA_MIGRATIONS_TABLE_SQL).execute(pool).await?;
    let max_version: Option<i64> =
        sqlx::query_scalar("SELECT MAX(version) FROM auth_schema_migrations").fetch_one(pool)
            .await?;
    check_schema_version(max_version)?;
    let current = max_version.unwrap_or(0);
    if current < 1 {
        apply_postgres_v1(pool).await?;
    }
    if current < 2 {
        apply_postgres_v2(pool).await?;
    }
    Ok(())
}

#[cfg(feature = "backend-postgres")]
async fn apply_postgres_v1(pool: &sqlx::PgPool) -> Result<()> {
    sqlx::query(super::USERS_TABLE_SQL).execute(pool).await?;
    sqlx::query(super::token_store::sqlite::TOKENS_TABLE_SQL).execute(pool).await?;
    sqlx::query(super::token_store::sqlite::TOKENS_INDEX_SQL).execute(pool).await?;
    sqlx::query(super::AUTH_COUNTERS_TABLE_SQL).execute(pool).await?;
    let now = unix_seconds() as i64;
    sqlx::query("INSERT INTO auth_schema_migrations (version, applied_at) VALUES (1, $1) ON CONFLICT (version) DO NOTHING")
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(feature = "backend-postgres")]
async fn apply_postgres_v2(pool: &sqlx::PgPool) -> Result<()> {
    sqlx::query("ALTER TABLE tokens ADD COLUMN IF NOT EXISTS name VARCHAR(255)").execute(pool)
        .await?;
    sqlx::query("ALTER TABLE tokens ADD COLUMN IF NOT EXISTS expires_at BIGINT").execute(pool)
        .await?;
    sqlx::query("ALTER TABLE tokens ADD COLUMN IF NOT EXISTS scopes TEXT").execute(pool).await?;
    sqlx::query("ALTER TABLE tokens ADD COLUMN IF NOT EXISTS kind VARCHAR(64)").execute(pool)
        .await?;
    let now = unix_seconds() as i64;
    sqlx::query("INSERT INTO auth_schema_migrations (version, applied_at) VALUES (2, $1) ON CONFLICT (version) DO NOTHING")
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

/// Run schema migrations on a `MySQL` database.
#[cfg(feature = "backend-mysql")]
pub(crate) async fn migrate_mysql(pool: &sqlx::MySqlPool) -> Result<()> {
    sqlx::query(SCHEMA_MIGRATIONS_TABLE_SQL).execute(pool).await?;
    let max_version: Option<i64> =
        sqlx::query_scalar("SELECT MAX(version) FROM auth_schema_migrations").fetch_one(pool)
            .await?;
    check_schema_version(max_version)?;
    let current = max_version.unwrap_or(0);
    if current < 1 {
        apply_mysql_v1(pool).await?;
    }
    if current < 2 {
        apply_mysql_v2(pool).await?;
    }
    Ok(())
}

#[cfg(feature = "backend-mysql")]
async fn apply_mysql_v1(pool: &sqlx::MySqlPool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            username     VARCHAR(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin PRIMARY KEY,
            bcrypt_hash  TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tokens (
            token_hash      CHAR(64) PRIMARY KEY,
            username        VARCHAR(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
            created_at      BIGINT NOT NULL,
            last_used_at    BIGINT NOT NULL,
            readonly        SMALLINT NOT NULL DEFAULT 0,
            cidr_whitelist  VARCHAR(4096) NOT NULL DEFAULT '[]',
            name            VARCHAR(255),
            expires_at      BIGINT
        )",
    )
    .execute(pool)
    .await?;
    create_token_index_mysql(pool).await?;
    sqlx::query(super::AUTH_COUNTERS_TABLE_SQL).execute(pool).await?;
    let now = unix_seconds() as i64;
    sqlx::query("INSERT IGNORE INTO auth_schema_migrations (version, applied_at) VALUES (1, ?)")
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(feature = "backend-mysql")]
async fn apply_mysql_v2(pool: &sqlx::MySqlPool) -> Result<()> {
    add_column_if_missing_mysql(pool, "ALTER TABLE tokens ADD COLUMN name VARCHAR(255)").await?;
    add_column_if_missing_mysql(pool, "ALTER TABLE tokens ADD COLUMN expires_at BIGINT").await?;
    add_column_if_missing_mysql(pool, "ALTER TABLE tokens ADD COLUMN scopes TEXT").await?;
    add_column_if_missing_mysql(pool, "ALTER TABLE tokens ADD COLUMN kind VARCHAR(64)").await?;
    sqlx::query(
        "ALTER TABLE users MODIFY username VARCHAR(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "ALTER TABLE tokens MODIFY username VARCHAR(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL",
    )
    .execute(pool)
    .await?;
    let now = unix_seconds() as i64;
    sqlx::query("INSERT IGNORE INTO auth_schema_migrations (version, applied_at) VALUES (2, ?)")
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(feature = "backend-mysql")]
async fn create_token_index_mysql(pool: &sqlx::MySqlPool) -> Result<()> {
    let result =
        sqlx::query("CREATE INDEX tokens_username ON tokens(username)").execute(pool).await;
    match result {
        Ok(_) => Ok(()),
        Err(err) if is_duplicate_index_mysql(&err) => Ok(()),
        Err(err) => Err(err.into()),
    }
}

#[cfg(feature = "backend-mysql")]
async fn add_column_if_missing_mysql(pool: &sqlx::MySqlPool, sql: &str) -> Result<()> {
    let result = sqlx::query(sql).execute(pool).await;
    match result {
        Ok(_) => Ok(()),
        Err(err) if is_duplicate_column_mysql(&err) => Ok(()),
        Err(err) => Err(err.into()),
    }
}

#[cfg(feature = "backend-mysql")]
fn is_duplicate_column_mysql(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .and_then(|err| err.try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>())
        .is_some_and(|err| err.number() == 1060)
}

#[cfg(feature = "backend-mysql")]
fn is_duplicate_index_mysql(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .and_then(|err| err.try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>())
        .is_some_and(|err| err.number() == 1061)
}
