use super::{Duration, RegistryError, Result};

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
    use sqlx::{ConnectOptions as _, Connection as _};
    let target = pnpr_config::redact_connection_url(target);
    let deadline = tokio::time::Instant::now() + timeout;
    let mut delay = Duration::from_millis(100);
    loop {
        let error = match tokio::time::timeout_at(
            deadline.min(tokio::time::Instant::now() + Duration::from_secs(10)),
            pool.connect_options().connect(),
        )
        .await
        {
            Ok(Ok(connection)) => {
                connection.close().await?;
                return Ok(());
            }
            Ok(Err(error)) => error,
            Err(_) => sqlx::Error::Io(std::io::ErrorKind::TimedOut.into()),
        };
        let cause = pnpr_error::redact_url_credentials(&error.to_string());
        if !is_retryable(&error) || tokio::time::Instant::now() + delay >= deadline {
            return Err(RegistryError::StorageUnavailable {
                reason: format!("database {target}: {cause}"),
            });
        }
        tracing::warn!(%target, error = %cause, ?delay, "database connection failed, retrying");
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(5));
    }
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
