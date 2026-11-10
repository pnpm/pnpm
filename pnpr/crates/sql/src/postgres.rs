use super::{Arc, Duration, Pools, Result, SqlBackendSettings, invalid_pool_size, timeout_millis};
use sqlx::{PgConnection, PgPool, postgres::PgPoolOptions};

/// Share a pool for these settings within the current runtime.
///
/// # Errors
///
/// The connection settings are invalid.
pub fn pool(settings: &SqlBackendSettings) -> Result<Arc<PgPool>> {
    static OPENED: Pools<sqlx::Postgres> = Pools::new();
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
) -> Result<PgPoolOptions> {
    let mut options = PgPoolOptions::new();
    if let Some(max_connections) = settings.max_connections {
        if max_connections == 0 {
            return Err(invalid_pool_size());
        }
        options = options.max_connections(max_connections);
    }
    options = options.after_connect(move |conn, _meta| Box::pin(configure(conn, session_timeout)));
    Ok(options.acquire_timeout(acquire_timeout))
}

/// Statements that set server-side operation deadlines.
#[must_use]
pub fn timeout_sql(timeout: Duration) -> String {
    format!("SET statement_timeout = {}", timeout_millis(timeout))
}

pub(crate) async fn configure(
    conn: &mut PgConnection,
    timeout: Duration,
) -> std::result::Result<(), sqlx::Error> {
    sqlx::query(&timeout_sql(timeout)).execute(conn).await?;
    Ok(())
}
