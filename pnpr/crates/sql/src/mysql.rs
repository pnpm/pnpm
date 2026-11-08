use super::{
    Arc, Duration, Pools, Result, SqlBackendSettings, invalid_pool_size, timeout_millis,
    timeout_seconds,
};
use sqlx::{MySqlPool, mysql::MySqlPoolOptions};

/// Share a pool for these settings within the current runtime.
///
/// # Errors
///
/// The connection settings are invalid.
pub fn pool(settings: &SqlBackendSettings) -> Result<Arc<MySqlPool>> {
    static OPENED: Pools<sqlx::MySql> = Pools::new();
    OPENED.get(settings, || {
        Ok(options(settings, settings.timeout, settings.timeout.max(settings.startup_timeout))?
            .connect_lazy(&settings.url)?)
    })
}

/// Pool options with server-side statement and lock deadlines.
///
/// # Errors
///
/// The configured pool size is zero.
pub fn options(
    settings: &SqlBackendSettings,
    session_timeout: Duration,
    acquire_timeout: Duration,
) -> Result<MySqlPoolOptions> {
    let mut options = MySqlPoolOptions::new();
    if let Some(max_connections) = settings.max_connections {
        if max_connections == 0 {
            return Err(invalid_pool_size());
        }
        options = options.max_connections(max_connections);
    }
    let timeout_sql = timeout_sql(session_timeout);
    options = options.after_connect(move |conn, _meta| {
        let timeout_sql = timeout_sql.clone();
        Box::pin(async move {
            sqlx::query("SET SESSION sql_mode = 'STRICT_ALL_TABLES,NO_ENGINE_SUBSTITUTION'")
                .execute(&mut *conn)
                .await?;
            sqlx::query(&timeout_sql).execute(conn).await?;
            Ok(())
        })
    });
    Ok(options.acquire_timeout(acquire_timeout))
}

/// Statements that set server-side operation deadlines.
#[must_use]
pub fn timeout_sql(timeout: Duration) -> String {
    format!(
        "SET SESSION max_execution_time = {}, innodb_lock_wait_timeout = {}, lock_wait_timeout = {}",
        timeout_millis(timeout),
        timeout_seconds(timeout),
        timeout_seconds(timeout),
    )
}
