use super::{
    Connection, Duration, LibsqlError, RegistryError, Result, TransactionBehavior, params,
};

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

/// Create the schema and seed the user counter. `sync` is
/// [`crate::schema::migrate_libsql`]'s.
pub(super) async fn init_schema<SyncFuture>(
    conn: &Connection,
    sync: impl Fn() -> SyncFuture + Send + Sync,
) -> Result<()>
where
    SyncFuture: Future<Output = Result<()>> + Send,
{
    crate::schema::migrate_libsql(conn, &sync).await?;
    ensure_user_counter(conn).await?;
    sync().await
}

pub(super) async fn ensure_user_counter(conn: &Connection) -> Result<()> {
    let mut rows = conn.query("SELECT COUNT(*) FROM users", ()).await?;
    let Some(row) = rows.next().await? else {
        return Err(missing_count_row());
    };
    let count: i64 = row.get(0)?;
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
    let mut counter_rows =
        tx.query("SELECT value FROM auth_counters WHERE name = ?1", params!["users"]).await?;
    let Some(counter_row) = counter_rows.next().await? else {
        drop(counter_rows);
        tx.commit().await?;
        return Ok(false);
    };
    let counter: i64 = counter_row.get(0)?;
    drop(counter_rows);
    let mut count_rows = tx.query("SELECT COUNT(*) FROM users", ()).await?;
    let Some(count_row) = count_rows.next().await? else {
        return Err(missing_count_row());
    };
    let count: i64 = count_row.get(0)?;
    drop(count_rows);
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

pub(super) fn is_transaction_conflict(error: &LibsqlError) -> bool {
    match error {
        LibsqlError::SqliteFailure(code, _) | LibsqlError::RemoteSqliteFailure(_, code, _) => {
            matches!(code & 0xff, 5 | 6)
        }
        LibsqlError::Hrana(error) => {
            let message = error.to_string();
            hrana_result_code(&message)
                .is_some_and(|code| {
                    matches!(code, "SQLITE_BUSY" | "SQLITE_LOCKED")
                        || code.starts_with("SQLITE_BUSY_")
                        || code.starts_with("SQLITE_LOCKED_")
                })
        }
        _ => false,
    }
}

pub(super) fn is_unique_violation(err: &LibsqlError) -> bool {
    match err {
        LibsqlError::SqliteFailure(code, message)
        | LibsqlError::RemoteSqliteFailure(_, code, message) => {
            *code == 19 || *code == 2067 || message.contains("UNIQUE constraint failed")
        }
        LibsqlError::Hrana(error) => {
            let message = error.to_string();
            message.contains("UNIQUE constraint failed")
                || hrana_result_code(&message)
                    .is_some_and(|code| {
                        matches!(code, "SQLITE_CONSTRAINT" | "SQLITE_CONSTRAINT_UNIQUE")
                    })
        }
        _ => false,
    }
}

/// The `SQLite` result code name a Hrana error carries. libsql does not expose
/// Hrana's structured error type publicly, so it is read from the message.
fn hrana_result_code(message: &str) -> Option<&str> {
    let (_, code) = message.rsplit_once(r#"code: ""#)?;
    let (code, _) = code.split_once('"')?;
    Some(code)
}

pub(super) fn missing_count_row() -> RegistryError {
    RegistryError::Internal { reason: "auth database COUNT(*) returned no rows".to_string() }
}
