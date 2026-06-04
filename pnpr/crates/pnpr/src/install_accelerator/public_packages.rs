//! Global set of **anonymously-readable** package names, so the per-user
//! grant table never gates a public package
//! ([pnpm/pnpm#12184](https://github.com/pnpm/pnpm/issues/12184)). A
//! forwarded token matching a registry only means pnpr fetched a package
//! with it, not that the package is private; in a mixed proxy that would
//! gate public content per user too. Populated lazily by one anonymous
//! probe per name, so a public package costs one round trip fleet-wide.
//! SQLite (WAL) like [`super::grant_table::GrantTable`]; best-effort.

use std::{
    path::Path,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::Connection;

/// Soft cap on classified names; the oldest rows (by `classified_at_ms`)
/// are evicted past this.
const MAX_ROWS: i64 = 100_000;

/// Concurrency-safe set of anonymously-readable package names.
pub(crate) struct PublicPackages {
    conn: Mutex<Connection>,
}

impl PublicPackages {
    /// Open (creating if needed) the classification database at `path`.
    pub(crate) fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS public_packages (
                 name             TEXT PRIMARY KEY,
                 classified_at_ms INTEGER NOT NULL
             );",
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Whether `name` was classified anonymously-readable within `ttl`
    /// (`None` = permanent). Keyed by name (readability is per-name).
    pub(crate) fn is_public(&self, name: &str, ttl: Option<Duration>) -> bool {
        let conn = self.conn.lock().expect("public packages poisoned");
        let classified_at: Option<i64> = conn
            .query_row(
                "SELECT classified_at_ms FROM public_packages WHERE name = ?1",
                rusqlite::params![name],
                |row| row.get(0),
            )
            .ok();
        let Some(classified_at) = classified_at else {
            return false;
        };
        match ttl {
            None => true,
            Some(ttl) => now_ms().saturating_sub(classified_at) <= ttl.as_millis() as i64,
        }
    }

    /// Record (or refresh) `name` as anonymously readable. Best-effort.
    pub(crate) fn record(&self, name: &str) {
        let now = now_ms();
        let conn = self.conn.lock().expect("public packages poisoned");
        let _ = conn.execute(
            "INSERT INTO public_packages (name, classified_at_ms) VALUES (?1, ?2)
             ON CONFLICT(name) DO UPDATE SET classified_at_ms = excluded.classified_at_ms",
            rusqlite::params![name, now],
        );
        evict_overflow(&conn);
    }
}

/// Trim the oldest rows past [`MAX_ROWS`], ordered by `classified_at_ms`.
fn evict_overflow(conn: &Connection) {
    let _ = conn.execute(
        "DELETE FROM public_packages WHERE name IN (
             SELECT name FROM public_packages
             ORDER BY classified_at_ms DESC
             LIMIT -1 OFFSET ?1
         )",
        rusqlite::params![MAX_ROWS],
    );
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
