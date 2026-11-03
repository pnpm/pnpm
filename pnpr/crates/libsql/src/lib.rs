//! How pnpr reaches a libSQL database, such as a Turso primary: the
//! connector it dials with, the pool its operations run on, and an embedded
//! replica that serves reads from a local file.

// Load pnpr-error, and the rusqlite its `server` feature brings, before
// libsql. The other order puts rusqlite's SQLite ahead of libsql's on the link
// line, and a test build that unifies that feature then fails on duplicate
// SQLite symbols. pnpr-auth and pnpr link in this order already.
extern crate pnpr_error;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use connector::{DialRetryGuard, LibsqlConnector};
pub use pool::{Checkout, ConnectionPool, first_row};

mod connector;
mod pool;

use connector::is_plain_http;
use libsql::{Builder, Database, Error as LibsqlError};
use pnpm_network::is_url_secure_for_credentials;
use pnpr_config::LibsqlSettings;
use pnpr_error::{RegistryError, Result};
use std::{future::Future, path::Path, time::Duration};

/// The connector for the database `settings` name, the settings of the
/// config block `section`, such as `backend.libsql`. An auth token must not
/// cross a cleartext link to a host other than loopback.
///
/// # Errors
///
/// The settings send a token over plain HTTP, or the platform trust store
/// cannot be loaded.
pub fn connector(settings: &LibsqlSettings, section: &str) -> Result<LibsqlConnector> {
    if settings.auth_token.is_some()
        && is_plain_http(&settings.url)
        && !is_url_secure_for_credentials(&settings.url)
    {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "{section}.authToken requires an https:// or libsql:// url, or http:// on a \
                 loopback host",
            ),
        });
    }
    Ok(LibsqlConnector::for_url(&settings.url, settings.auth_token.is_some())?)
}

/// Open the database `settings` name, dialed with `connector`: an embedded
/// replica at `replicaPath` when it is set, otherwise the remote primary.
/// The connector refuses to send the settings' auth token, if they have one,
/// over plain HTTP to a host other than loopback, however it was built.
///
/// A replica runs its initial sync here. With a `syncInterval`, libsql keeps
/// it current in the background. libsql probes the primary for its newer sync
/// protocol, the only one Turso Cloud serves, and falls back to the
/// replication protocol of a self-hosted `sqld`. Opening a connection over the
/// newer one blocks in place, so the caller must run on tokio's multi-thread
/// runtime.
///
/// # Errors
///
/// The database cannot be reached, or the replica cannot be created.
pub async fn open(settings: &LibsqlSettings, connector: LibsqlConnector) -> Result<Database> {
    if let Some(path) = &settings.replica_path {
        return open_replica(path, settings, connector).await;
    }
    open_primary(settings, connector).await
}

/// Open the remote primary `settings` name, ignoring any `replicaPath`.
///
/// # Errors
///
/// The database URL cannot be used.
pub async fn open_primary(
    settings: &LibsqlSettings,
    connector: LibsqlConnector,
) -> Result<Database> {
    let connector = connector.carrying_token(settings.auth_token.is_some());
    let auth_token = settings.auth_token.clone().unwrap_or_default();
    let database = Box::pin(
        Builder::new_remote(settings.url.clone(), auth_token).connector(connector).build(),
    )
    .await?;
    Ok(database)
}

/// Open the embedded replica at `path`, creating its directory the way the
/// local stores create theirs.
async fn open_replica(
    path: &Path,
    settings: &LibsqlSettings,
    connector: LibsqlConnector,
) -> Result<Database> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        tokio::fs::create_dir_all(parent).await.map_err(RegistryError::Io)?;
    }
    let connector = connector.carrying_token(settings.auth_token.is_some());
    let auth_token = settings.auth_token.clone().unwrap_or_default();
    // A caller brings its own writes into the replica with `Database::sync`.
    let mut builder = Builder::new_remote_replica(path, settings.url.clone(), auth_token)
        .connector(connector)
        .read_your_writes(false);
    let interval =
        settings.sync_interval_secs.unwrap_or(LibsqlSettings::DEFAULT_SYNC_INTERVAL_SECS);
    if interval > 0 {
        builder = builder.sync_interval(Duration::from_secs(interval));
    }
    let db = Box::pin(builder.build()).await?;
    if interval == 0 {
        // libsql bootstraps the replica during the build only alongside a
        // sync interval; otherwise it would do so inside the blocking
        // `Database::connect`, which a startup deadline cannot cancel.
        Box::pin(db.sync()).await?;
    }
    Ok(db)
}

/// Run `future` under `deadline`, failing with the error `timed_out` returns
/// once it passes. Use only around reads and the wait before a write: a
/// write cut off after it reached the database leaves its commit state
/// unknown.
pub(crate) async fn with_deadline<Loaded, DbError>(
    deadline: Duration,
    timed_out: fn() -> RegistryError,
    future: impl Future<Output = std::result::Result<Loaded, DbError>>,
) -> Result<Loaded>
where
    RegistryError: From<DbError>,
{
    match tokio::time::timeout(deadline, future).await {
        Ok(result) => result.map_err(RegistryError::from),
        Err(_) => Err(timed_out()),
    }
}

/// Whether `error` is a lock conflict with another writer, after which the
/// whole transaction may run again.
#[must_use]
pub fn is_transaction_conflict(error: &LibsqlError) -> bool {
    match error {
        LibsqlError::SqliteFailure(code, _) | LibsqlError::RemoteSqliteFailure(_, code, _) => {
            matches!(code & 0xff, 5 | 6)
        }
        LibsqlError::Hrana(error) => {
            let message = error.to_string();
            let coded = hrana_result_code(&message)
                .is_some_and(|code| {
                    matches!(code, "SQLITE_BUSY" | "SQLITE_LOCKED")
                        || code.starts_with("SQLITE_BUSY_")
                        || code.starts_with("SQLITE_LOCKED_")
                });
            // A failed step of a batch reaches the client as its message
            // alone, `SQLite`'s own text for these codes.
            coded
                || message.contains("database is locked")
                || message.contains("database table is locked")
        }
        _ => false,
    }
}

/// Whether `error` is a violated uniqueness constraint.
#[must_use]
pub fn is_unique_violation(error: &LibsqlError) -> bool {
    match error {
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

#[cfg(test)]
mod tests;
