use crate::token_store::unix_seconds;
use pnpr_error::{RegistryError, Result};
use rusqlite::OptionalExtension as _;
use std::future::Future;

/// Current version of the auth database schema.
pub const CURRENT_SCHEMA_VERSION: i64 = 3;

/// Table storing schema migrations applied to the auth database.
pub(crate) const SCHEMA_MIGRATIONS_TABLE_SQL: &str =
    "CREATE TABLE IF NOT EXISTS auth_schema_migrations (
        version     BIGINT PRIMARY KEY,
        applied_at  BIGINT NOT NULL
    )";

fn check_schema_version(max_version: Option<i64>) -> Result<()> {
    if let Some(version) = max_version
        && version != CURRENT_SCHEMA_VERSION
    {
        return Err(RegistryError::Internal {
            reason: format!(
                "auth database schema version {version} is incompatible with version {CURRENT_SCHEMA_VERSION}; create a new auth database for this experimental release",
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
    if current == 0 {
        apply_sqlite_v1(conn)?;
    }
    Ok(())
}

fn apply_sqlite_v1(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute(super::token_store::sqlite::TOKENS_TABLE_SQL, [])?;
    conn.execute(super::token_store::sqlite::TOKENS_INDEX_SQL, [])?;
    let now = unix_seconds() as i64;
    conn.execute(
        "INSERT INTO auth_schema_migrations (version, applied_at) VALUES (3, ?1) ON CONFLICT (version) DO NOTHING",
        rusqlite::params![now],
    )?;
    Ok(())
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
    if current == 0 {
        apply_libsql_v1(conn).await?;
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
        "INSERT INTO auth_schema_migrations (version, applied_at) VALUES (3, ?1) ON CONFLICT (version) DO NOTHING",
        libsql::params![now],
    )
    .await?;
    Ok(())
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
    if current == 0 {
        apply_postgres_v1(pool).await?;
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
    sqlx::query("INSERT INTO auth_schema_migrations (version, applied_at) VALUES (3, $1) ON CONFLICT (version) DO NOTHING")
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
    if current == 0 {
        apply_mysql_v1(pool).await?;
    }
    Ok(())
}

#[cfg(feature = "backend-mysql")]
async fn apply_mysql_v1(pool: &sqlx::MySqlPool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            id           VARCHAR(64) PRIMARY KEY,
            username     VARCHAR(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin UNIQUE NOT NULL,
            bcrypt_hash  TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tokens (
            token_hash      CHAR(64) PRIMARY KEY,
            user_id         VARCHAR(64) NOT NULL,
            created_at      BIGINT NOT NULL,
            last_used_at    BIGINT NOT NULL,
            readonly        SMALLINT NOT NULL DEFAULT 0,
            cidr_whitelist  VARCHAR(4096) NOT NULL DEFAULT '[]',
            name            VARCHAR(255),
            expires_at      BIGINT,
            scopes          TEXT,
            kind            VARCHAR(64)
        )",
    )
    .execute(pool)
    .await?;
    create_token_index_mysql(pool).await?;
    sqlx::query(super::AUTH_COUNTERS_TABLE_SQL).execute(pool).await?;
    let now = unix_seconds() as i64;
    sqlx::query("INSERT IGNORE INTO auth_schema_migrations (version, applied_at) VALUES (3, ?)")
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(feature = "backend-mysql")]
async fn create_token_index_mysql(pool: &sqlx::MySqlPool) -> Result<()> {
    let result = sqlx::query("CREATE INDEX tokens_user_id ON tokens(user_id)").execute(pool).await;
    match result {
        Ok(_) => Ok(()),
        Err(err) if is_duplicate_index_mysql(&err) => Ok(()),
        Err(err) => Err(err.into()),
    }
}

#[cfg(feature = "backend-mysql")]
fn is_duplicate_index_mysql(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .and_then(|err| err.try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>())
        .is_some_and(|err| err.number() == 1061)
}
