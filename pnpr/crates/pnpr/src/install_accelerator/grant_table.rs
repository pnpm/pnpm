//! Per-user access grants for externally-resolved private content
//! ([pnpm/pnpm#12184](https://github.com/pnpm/pnpm/issues/12184)).
//!
//! When the install accelerator fetches a package from an **external**
//! registry with the caller's forwarded credentials, the package carries
//! no pnpr `packages:` policy — its authority is that registry, per user.
//! The store deduplicates the bytes globally, but possession of the bytes
//! must not authorize a user the upstream never cleared. This table is
//! the small per-`(user, name@version)` allow-list that closes the gap:
//!
//! * A grant is recorded the moment the upstream accepts the caller's
//!   token for a version (a cold fetch, or an explicit re-verify).
//! * A later **cache hit** for the same `(user, version)` is served
//!   straight from the grant — no upstream round trip.
//! * **Clear-on-discovery:** a `401`/`403` from the upstream *as that
//!   user* purges every grant the user holds for the package, so a lost
//!   entitlement stops authorizing already-cached versions.
//!
//! Backed by SQLite (WAL) like [`super::verdict_cache::VerdictCache`], so
//! many client connections share the file and the server can evict
//! without append/compaction races. Every method is best-effort: a DB
//! error never fails the request — the worst case is one extra re-verify.

use std::{
    path::Path,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::Connection;

/// Soft cap on stored grants; the oldest rows (by `granted_at_ms`) are
/// evicted past this. Generous — each row is two short strings and a
/// timestamp.
const MAX_ROWS: i64 = 100_000;

/// Concurrency-safe store of per-`(user, name@version)` access grants.
pub(crate) struct GrantTable {
    conn: Mutex<Connection>,
}

impl GrantTable {
    /// Open (creating if needed) the grant database at `path`.
    pub(crate) fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS grants (
                 user          TEXT NOT NULL,
                 pkg           TEXT NOT NULL,
                 granted_at_ms INTEGER NOT NULL,
                 PRIMARY KEY (user, pkg)
             );",
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Whether `(user, pkg)` holds a grant still within `ttl`. `ttl` of
    /// `None` means permanent (a grant never expires — revocation then
    /// relies on clear-on-discovery alone). `pkg` is the `name@version`
    /// package id.
    pub(crate) fn is_granted(&self, user: &str, pkg: &str, ttl: Option<Duration>) -> bool {
        let conn = self.conn.lock().expect("grant table poisoned");
        let granted_at: Option<i64> = conn
            .query_row(
                "SELECT granted_at_ms FROM grants WHERE user = ?1 AND pkg = ?2",
                rusqlite::params![user, pkg],
                |row| row.get(0),
            )
            .ok();
        let Some(granted_at) = granted_at else {
            return false;
        };
        match ttl {
            None => true,
            Some(ttl) => now_ms().saturating_sub(granted_at) <= ttl.as_millis() as i64,
        }
    }

    /// Record (or refresh) a grant for `(user, pkg)`. Best-effort.
    pub(crate) fn record(&self, user: &str, pkg: &str) {
        let now = now_ms();
        let conn = self.conn.lock().expect("grant table poisoned");
        let _ = conn.execute(
            "INSERT INTO grants (user, pkg, granted_at_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(user, pkg) DO UPDATE SET granted_at_ms = excluded.granted_at_ms",
            rusqlite::params![user, pkg, now],
        );
        evict_overflow(&conn);
    }

    /// Clear-on-discovery: drop every grant `user` holds for package
    /// `name`, across all versions. Rows key `pkg` as `name@version`, so
    /// this matches by the `name@` prefix via `substr` (LIKE would need
    /// escaping for names carrying `_`/`%`). Best-effort.
    pub(crate) fn clear_package(&self, user: &str, name: &str) {
        let with_at = format!("{name}@");
        let prefix_len = with_at.chars().count() as i64;
        let conn = self.conn.lock().expect("grant table poisoned");
        let _ = conn.execute(
            "DELETE FROM grants WHERE user = ?1 AND substr(pkg, 1, ?2) = ?3",
            rusqlite::params![user, prefix_len, with_at],
        );
    }
}

/// Trim the oldest rows past [`MAX_ROWS`], ordered by `granted_at_ms`.
fn evict_overflow(conn: &Connection) {
    let _ = conn.execute(
        "DELETE FROM grants WHERE rowid IN (
             SELECT rowid FROM grants
             ORDER BY granted_at_ms DESC
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
