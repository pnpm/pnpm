use super::{Arc, Duration, Pools, Result, SqlBackendSettings, invalid_pool_size, timeout_millis};
use sqlx::{PgPool, postgres::PgPoolOptions};

/// Share a pool for these settings within the current runtime.
///
/// # Errors
///
/// The connection settings are invalid.
pub fn pool(settings: &SqlBackendSettings) -> Result<Arc<PgPool>> {
    static OPENED: Pools<sqlx::Postgres> = Pools::new();
    OPENED.get(settings, || {
        Ok(options(settings, settings.timeout, settings.timeout)?.connect_lazy(&settings.url)?)
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
) -> Result<PgPoolOptions> {
    let mut options = PgPoolOptions::new();
    if let Some(max_connections) = settings.max_connections {
        if max_connections == 0 {
            return Err(invalid_pool_size());
        }
        options = options.max_connections(max_connections);
    }
    let statement_timeout_sql = timeout_sql(session_timeout);
    options = options.after_connect(move |conn, _meta| {
        let statement_timeout_sql = statement_timeout_sql.clone();
        Box::pin(async move {
            sqlx::query(&statement_timeout_sql).execute(conn).await?;
            Ok(())
        })
    });
    Ok(options.acquire_timeout(acquire_timeout))
}

/// Statements that set server-side operation deadlines.
#[must_use]
pub fn timeout_sql(timeout: Duration) -> String {
    format!("SET statement_timeout = {}", timeout_millis(timeout))
}
