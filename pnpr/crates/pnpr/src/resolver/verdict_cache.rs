//! SQLite-backed whole-lockfile verification verdict cache for the pnpr
//! resolver ([pnpm/pnpm#12139](https://github.com/pnpm/pnpm/issues/12139)).
//!
//! Caches the *result* of verifying an entire input lockfile. Like the
//! local `lockfile-verified.jsonl` cache
//! ([`pacquet_lockfile_verification::CacheRecord`]), a row is keyed by
//! the lockfile content hash and stores the merged policy snapshot; a
//! lookup is a hit only when every active verifier's
//! [`ResolutionVerifier::can_trust_past_check`] accepts that stored
//! snapshot (a looser current policy can trust a stricter cached run).
//! One row per hash — a later pass overwrites the snapshot, matching the
//! local cache's last-write-wins-by-hash rather than keeping a row per
//! policy. Backed by `SQLite` so many client connections can read/write
//! concurrently and the server can evict without the JSONL
//! append/compaction races.
//!
//! Only *passes* are cached. An age-pass is monotonic (versions only get
//! older) and the lockfile hash pins the exact versions, so a cached
//! pass stays time-correct without storing a cutoff — the same reason
//! the local cache is correct via `can_trust_past_check` alone. A hit is
//! O(1): one lookup by hash, then the policy-trust check.
//!
//! Deliberately whole-lockfile, not per-entry: the expensive fetches are
//! already covered by the warm packument cache, so per-entry keying would
//! cost O(N) lookups per install for a negligible recompute saving.
//!
//! [`ResolutionVerifier::can_trust_past_check`]: pacquet_resolving_resolver_base::ResolutionVerifier::can_trust_past_check

use std::{
    path::Path,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::Connection;
use serde_json::{Map, Value};

/// Soft cap on cached verdicts; the oldest rows (by `verified_at_ms`)
/// are evicted past this. Generous — each row is a hash plus a small
/// policy JSON blob.
const MAX_ROWS: i64 = 10_000;

/// Concurrency-safe store of whole-lockfile verification verdicts.
pub(crate) struct VerdictCache {
    conn: Mutex<Connection>,
}

impl VerdictCache {
    /// Open (creating if needed) the verdict database at `path`. WAL
    /// mode + a busy timeout let separate processes sharing the file
    /// coexist; within one server process the `Mutex` serializes access.
    pub(crate) fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS lockfile_verdicts (
                 hash           TEXT PRIMARY KEY,
                 policy         TEXT NOT NULL,
                 verified_at_ms INTEGER NOT NULL
             );",
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Return `true` when a prior pass for `hash` is recorded under a
    /// policy the caller still trusts — `trusts(cached_policy)` is the
    /// merged `can_trust_past_check` consensus across the active
    /// verifiers. A miss (no row, an unreadable policy, or a policy no
    /// longer trusted) returns `false`.
    pub(crate) fn is_verified(
        &self,
        hash: &str,
        trusts: impl Fn(&Map<String, Value>) -> bool,
    ) -> bool {
        let conn = self.conn.lock().expect("verdict cache poisoned");
        let policy_json: Option<String> = conn
            .query_row(
                "SELECT policy FROM lockfile_verdicts WHERE hash = ?1",
                rusqlite::params![hash],
                |row| row.get(0),
            )
            .ok();
        let Some(policy_json) = policy_json else {
            return false;
        };
        let Ok(policy) = serde_json::from_str::<Map<String, Value>>(&policy_json) else {
            // A corrupt policy blob would miss forever; drop the row so
            // the next install re-verifies and re-records a clean one.
            let _ = conn
                .execute("DELETE FROM lockfile_verdicts WHERE hash = ?1", rusqlite::params![hash]);
            return false;
        };
        trusts(&policy)
    }

    /// Record a successful whole-lockfile verification under the merged
    /// policy snapshot. Best-effort: a DB error is swallowed (the next
    /// install just re-verifies). Callers must record only *passes*.
    pub(crate) fn record(&self, hash: &str, policy: &Map<String, Value>) {
        let policy_json = Value::Object(policy.clone()).to_string();
        let now_ms = now_ms();
        let conn = self.conn.lock().expect("verdict cache poisoned");
        let _ = conn.execute(
            "INSERT INTO lockfile_verdicts (hash, policy, verified_at_ms)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(hash) DO UPDATE SET
                policy = excluded.policy,
                verified_at_ms = excluded.verified_at_ms",
            rusqlite::params![hash, policy_json, now_ms],
        );
        evict_overflow(&conn);
    }
}

/// Trim the oldest rows past [`MAX_ROWS`], ordered by `verified_at_ms`
/// — i.e. by last-verification time (set on `record`, refreshed when a
/// hash is re-recorded), not access time: a cache *hit* deliberately
/// doesn't rewrite the row, keeping `is_verified` a pure read. No TTL
/// because a cached pass never goes stale (monotonic age + the hash pins
/// exact versions), so eviction is purely space management.
fn evict_overflow(conn: &Connection) {
    let _ = conn.execute(
        "DELETE FROM lockfile_verdicts WHERE hash IN (
             SELECT hash FROM lockfile_verdicts
             ORDER BY verified_at_ms DESC
             LIMIT -1 OFFSET ?1
         )",
        rusqlite::params![MAX_ROWS],
    );
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_millis() as i64)
}

#[cfg(test)]
mod tests;
