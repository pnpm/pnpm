use super::{Duration, RegistryError, Result};
use sqlx::{ConnectOptions as _, Connection as _};

/// Check initial connectivity with visible retries before the pool takes over.
///
/// # Errors
///
/// The database rejects the connection or the startup deadline expires.
pub async fn check_connection<Database: sqlx::Database>(
    pool: &sqlx::Pool<Database>,
    target: &str,
    timeout: Duration,
) -> Result<()> {
    let target = pnpr_config::redact_connection_url(target);
    let deadline = tokio::time::Instant::now() + timeout;
    let mut delay = Duration::from_millis(100);
    loop {
        let error = match connect_once(pool, deadline).await {
            Ok(connection) => {
                connection.close().await?;
                return Ok(());
            }
            Err(error) => error,
        };
        let cause = pnpr_error::redact_url_credentials(&error.to_string());
        let failure =
            RegistryError::StorageUnavailable { reason: format!("database {target}: {cause}") };
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if !is_retryable(&error) || remaining.is_zero() {
            return Err(failure);
        }
        let wait = delay.min(remaining);
        tracing::warn!(%target, error = %cause, delay = ?wait, "database connection failed, retrying");
        tokio::time::sleep(wait).await;
        if tokio::time::Instant::now() >= deadline {
            return Err(failure);
        }
        delay = (delay * 2).min(Duration::from_secs(5));
    }
}

async fn connect_once<Database: sqlx::Database>(
    pool: &sqlx::Pool<Database>,
    deadline: tokio::time::Instant,
) -> std::result::Result<Database::Connection, sqlx::Error> {
    tokio::time::timeout_at(
        deadline.min(tokio::time::Instant::now() + Duration::from_secs(10)),
        pool.connect_options().connect(),
    )
    .await
    .map_err(|_| sqlx::Error::Io(std::io::ErrorKind::TimedOut.into()))?
}

fn is_retryable(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Io(error) => matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::TimedOut
        ),
        sqlx::Error::Database(error) => error.is_transient_in_connect_phase(),
        _ => false,
    }
}
