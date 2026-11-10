use super::{Duration, RegistryError, Result};
use sqlx::{ConnectOptions as _, pool::PoolConnection};
use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};
use tokio::sync::{Mutex, OwnedMutexGuard};

/// Open the startup connection with visible retries.
/// If the deadline cancels a retry, the error retains the preceding connection failure.
///
/// # Errors
///
/// The database rejects the connection or the startup deadline expires.
pub async fn connect_startup<Database: sqlx::Database>(
    pool: &sqlx::Pool<Database>,
    target: &str,
    timeout: Duration,
) -> Result<Database::Connection> {
    let deadline = tokio::time::Instant::now() + timeout;
    connect_with_retry(target, deadline, || {
        let options = pool.connect_options();
        async move { options.connect().await }
    })
    .await
}

async fn connect_with_retry<Connection, Connect, Connecting>(
    target: &str,
    deadline: tokio::time::Instant,
    mut connect: Connect,
) -> Result<Connection>
where
    Connect: FnMut() -> Connecting,
    Connecting: std::future::Future<Output = std::result::Result<Connection, sqlx::Error>>,
{
    let target = pnpr_config::redact_connection_url(target);
    let mut delay = Duration::from_millis(100);
    let mut previous_failure = None;
    loop {
        let error = match connect_once(connect(), deadline).await {
            Ok(connection) => return Ok(connection),
            Err(error) => error.into_driver_error(deadline, previous_failure.take())?,
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
        previous_failure = Some(failure);
        delay = (delay * 2).min(Duration::from_secs(5));
    }
}

async fn connect_once<Connection>(
    connecting: impl std::future::Future<Output = std::result::Result<Connection, sqlx::Error>>,
    deadline: tokio::time::Instant,
) -> std::result::Result<Connection, ConnectError> {
    tokio::time::timeout_at(
        deadline.min(tokio::time::Instant::now() + Duration::from_secs(10)),
        connecting,
    )
    .await
    .map_err(|_| ConnectError::TimedOut)?
    .map_err(ConnectError::Driver)
}

enum ConnectError {
    Driver(sqlx::Error),
    TimedOut,
}

impl ConnectError {
    fn into_driver_error(
        self,
        deadline: tokio::time::Instant,
        previous_failure: Option<RegistryError>,
    ) -> Result<sqlx::Error> {
        match self {
            Self::Driver(error) => Ok(error),
            Self::TimedOut => {
                if tokio::time::Instant::now() >= deadline
                    && let Some(failure) = previous_failure
                {
                    return Err(failure);
                }
                Ok(sqlx::Error::Io(std::io::ErrorKind::TimedOut.into()))
            }
        }
    }
}

fn is_retryable(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Io(error) => matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::TimedOut,
        ),
        sqlx::Error::Database(error) => error.is_transient_in_connect_phase(),
        _ => false,
    }
}

/// The connection shared by schema setup and auth counter initialization.
pub type StartupConnection<Database> = Arc<Mutex<Option<<Database as sqlx::Database>::Connection>>>;

/// A request's pooled connection or the connection held during startup.
pub enum Checkout<Database: sqlx::Database> {
    Pooled(PoolConnection<Database>),
    Startup { connection: OwnedMutexGuard<Option<Database::Connection>>, close: bool },
}

impl<Database: sqlx::Database> Checkout<Database> {
    /// Borrow the startup connection under the caller's deadline.
    ///
    /// # Errors
    ///
    /// Checkout times out or a failed initialization closed the connection.
    pub async fn startup(
        connection: StartupConnection<Database>,
        timeout: Duration,
    ) -> Result<Self> {
        let connection = tokio::time::timeout(timeout, connection.lock_owned())
            .await
            .map_err(|_| sqlx::Error::PoolTimedOut)?;
        if connection.is_none() {
            return Err(RegistryError::Internal { reason: "startup connection is closed".into() });
        }
        Ok(Self::Startup { connection, close: false })
    }

    pub(crate) const fn is_startup(&self) -> bool {
        matches!(self, Self::Startup { .. })
    }

    pub(crate) fn close_on_drop(&mut self) {
        match self {
            Self::Pooled(connection) => connection.close_on_drop(),
            Self::Startup { close, .. } => *close = true,
        }
    }
}

impl<Database: sqlx::Database> Deref for Checkout<Database> {
    type Target = Database::Connection;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Pooled(connection) => connection,
            Self::Startup { connection, .. } => {
                connection.as_ref().expect("startup connection is open")
            }
        }
    }
}

impl<Database: sqlx::Database> DerefMut for Checkout<Database> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Pooled(connection) => connection,
            Self::Startup { connection, .. } => {
                connection.as_mut().expect("startup connection is open")
            }
        }
    }
}

impl<Database: sqlx::Database> Drop for Checkout<Database> {
    fn drop(&mut self) {
        if let Self::Startup { connection, close: true } = self {
            drop(connection.take());
        }
    }
}

#[cfg(test)]
mod tests;
