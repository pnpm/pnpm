//! Initialize a shared index even when another installer is switching it to WAL.
use rusqlite::{Connection, ErrorCode};
use std::time::Duration;

/// `SQLite` can return `SQLITE_BUSY` from a journal-mode transition without
/// invoking its busy handler. Retry the idempotent setup within the same
/// pnpm v11 retry policy: up to 100 retries, 50 milliseconds apart.
pub(super) fn initialize(connection: &Connection) -> rusqlite::Result<()> {
    connection.busy_timeout(Duration::from_secs(5))?;
    let mut retries = 0;
    loop {
        let result = connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            PRAGMA mmap_size=536870912;
            PRAGMA cache_size=-32000;
            PRAGMA temp_store=MEMORY;
            PRAGMA wal_autocheckpoint=10000;
            CREATE TABLE IF NOT EXISTS package_index (
              key TEXT PRIMARY KEY,
              data BLOB NOT NULL
            ) WITHOUT ROWID;",
            )
            .and_then(|()| connection.execute_batch(super::aliases::CREATE_ALIAS_TABLE));
        match result {
            Err(error)
                if error.sqlite_error_code() == Some(ErrorCode::DatabaseBusy) && retries < 100 =>
            {
                retries += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests;
