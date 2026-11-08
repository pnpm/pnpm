use super::{
    Connection, Duration, RegistryError, Result, TransactionBehavior, first_row,
    is_unique_violation, params,
};
use pnpr_libsql::is_transaction_conflict;

/// Take one slot of the capped user counter, reporting whether the cap left
/// one to take.
pub(super) async fn claim_user_counter_slot(tx: &libsql::Transaction, max: u64) -> Result<bool> {
    let sql_max = i64::try_from(max)
        .map_err(|_| RegistryError::InvalidConfig {
            reason: "backend.libsql auth max_users must fit a signed BIGINT".to_string(),
        })?;
    let updated = tx.execute(
        "UPDATE auth_counters SET value = value + 1
                         WHERE name = ?1 AND value < ?2",
        params!["users", sql_max],
    )
    .await?;
    Ok(updated > 0)
}

/// Create the schema and seed the user counter.
pub(super) async fn init_schema<SyncFuture>(
    conn: &Connection,
    sync: impl Fn() -> SyncFuture + Send + Sync,
) -> Result<()>
where
    SyncFuture: Future<Output = Result<()>> + Send,
{
    pnpr_sql::libsql::ensure_schema(conn, "libSQL auth database").await?;
    sync().await?;
    ensure_user_counter(conn).await?;
    sync().await
}

pub(super) async fn ensure_user_counter(conn: &Connection) -> Result<()> {
    let rows = conn.query("SELECT COUNT(*) FROM users WHERE bcrypt_hash <> '!'", ()).await?;
    let Some(count) = first_row(rows, |row| Ok(row.get::<i64>(0)?)).await? else {
        return Err(missing_count_row());
    };
    let tx = conn.transaction().await?;
    let inserted = tx.execute(
        "INSERT INTO auth_counters (name, value) VALUES (?1, ?2)",
        params!["users", count],
    )
    .await;
    match inserted {
        Ok(_) => {}
        Err(err) if is_unique_violation(&err) => {}
        Err(err) => {
            tx.rollback().await?;
            return Err(err.into());
        }
    }
    tx.execute(
        "UPDATE auth_counters
         SET value = CASE WHEN value < ?2 THEN ?2 ELSE value END
         WHERE name = ?1",
        params!["users", count],
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub(super) async fn reconcile_user_counter_overcount(conn: &Connection) -> Result<bool> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).await?;
    let counter_rows =
        tx.query("SELECT value FROM auth_counters WHERE name = ?1", params!["users"]).await?;
    let Some(counter) = first_row(counter_rows, |row| Ok(row.get::<i64>(0)?)).await? else {
        tx.commit().await?;
        return Ok(false);
    };
    let count_rows = tx.query("SELECT COUNT(*) FROM users WHERE bcrypt_hash <> '!'", ()).await?;
    let Some(count) = first_row(count_rows, |row| Ok(row.get::<i64>(0)?)).await? else {
        return Err(missing_count_row());
    };
    if counter <= count {
        tx.commit().await?;
        return Ok(false);
    }
    tx.execute(
        "UPDATE auth_counters SET value = ?2 WHERE name = ?1",
        params!["users", count.max(0)],
    )
    .await?;
    tx.commit().await?;
    Ok(true)
}

pub(super) async fn retry_database_conflicts<Value, Operation, Pending>(
    mut operation: Operation,
) -> Result<Value>
where
    Operation: FnMut() -> Pending,
    Pending: std::future::Future<Output = Result<Value>>,
{
    let mut retries = 0;
    loop {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(RegistryError::Libsql(error)) if retries < 8 && is_transaction_conflict(&error) => {
                tokio::time::sleep(Duration::from_millis(10 << retries.min(5))).await;
                retries += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

pub(super) fn missing_count_row() -> RegistryError {
    RegistryError::Internal { reason: "auth database COUNT(*) returned no rows".to_string() }
}
