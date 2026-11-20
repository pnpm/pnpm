//! SQLite-backed whole-lockfile verification verdict cache for the pnpr
//! resolver ([pnpm/pnpm#12139](https://github.com/pnpm/pnpm/issues/12139)).
//!
//! Caches the *result* of verifying an entire input lockfile, scoped by the
//! private-route [`Footprint`] verification touched and the stored
//! `(graph, grants)` epoch. Like the local `lockfile-verified.jsonl` cache
//! ([`pnpm_lockfile_verification::CacheRecord`]), each row stores the merged
//! policy snapshot; a lookup is a hit only when the caller is authorized for
//! every private route in the stored [`Footprint`] and every active verifier's
//! [`ResolutionVerifier::can_trust_past_check`] accepts that stored snapshot
//! (a looser current policy can trust a stricter cached run).
//!
//! Rows are keyed by `(hash, epoch, footprint_key)` so callers resolving the
//! same lockfile through distinct private upstream aliases each keep their own
//! cached verdict without overwriting one another, an earlier-epoch verification
//! cannot clobber a pass recorded in the current epoch, and callers sharing the
//! same access reuse one row. Backed by `SQLite` so many client connections
//! can read/write concurrently and the server can evict without the JSONL
//! append/compaction races.
//!
//! Only *passes* are cached. An age-pass is monotonic (versions only get
//! older) and the lockfile hash pins the exact versions, so a cached pass
//! stays time-correct while the grant and graph epoch is unchanged and the
//! caller still satisfies the [`Footprint`].
//!
//! Deliberately whole-lockfile, not per-entry: the expensive fetches are
//! already covered by the warm packument cache, so per-entry keying would
//! cost O(N) lookups per install for a negligible recompute saving.
//!
//! [`ResolutionVerifier::can_trust_past_check`]: pnpm_resolving_resolver_base::ResolutionVerifier::can_trust_past_check

use std::{
    path::Path,
    sync::{
        Mutex,
        PoisonError,
    },
    time::{
        Duration,
        SystemTime,
        UNIX_EPOCH,
    },
};

use pnpr_policy::Identity;
use pnpr_route::{
    Footprint,
    RouteContext,
};
use rusqlite::Connection;
use serde_json::{
    Map,
    Value,
};

/// Soft cap on cached verdicts; the oldest rows (by `verified_at_ms`)
/// are evicted past this.
const MAX_ROWS: i64 = 10_000;

/// Cap on distinct private-footprint candidates retained for a single lockfile
/// hash, mirroring `MAX_RESOLUTION_CACHE_CANDIDATES_PER_KEY`.
const MAX_CANDIDATES_PER_HASH: i64 = 8;

/// Inputs to a [`VerdictCache::verified_footprint`] lookup.
pub(crate) struct VerdictLookup<'a> {
    pub(crate) hash: &'a str,
    pub(crate) epoch: &'a str,
    pub(crate) secret: &'a [u8],
    pub(crate) route_context: &'a RouteContext,
    pub(crate) identity: &'a Identity,
}

/// Inputs to [`VerdictCache::record`].
pub(crate) struct VerdictRecord<'a> {
    pub(crate) hash: &'a str,
    pub(crate) footprint: &'a Footprint,
    pub(crate) secret: &'a [u8],
    pub(crate) epoch: &'a str,
    pub(crate) policy: Map<String, Value>,
}

struct StoredVerdict {
    footprint_key: String,
    footprint_json: String,
    policy_json: String,
}

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
                 hash           TEXT NOT NULL,
                 footprint_key  TEXT NOT NULL,
                 footprint      TEXT NOT NULL,
                 policy         TEXT NOT NULL,
                 epoch          TEXT NOT NULL,
                 verified_at_ms INTEGER NOT NULL,
                 PRIMARY KEY (hash, epoch, footprint_key)
             );",
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Drop every cached verdict. Called when the registry graph or its grants
    /// change so a verdict recorded as public under the prior policy cannot
    /// satisfy callers the new policy restricts.
    pub(crate) fn clear(&self) {
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = conn.execute("DELETE FROM lockfile_verdicts", []);
    }

    /// Return the matched [`Footprint`] when a prior pass for `lookup.hash` is
    /// recorded in the current `lookup.epoch`, `lookup.identity` satisfies
    /// every private route in that footprint, and `trusts(cached_policy)`
    /// accepts the stored policy snapshot.
    pub(crate) fn verified_footprint(
        &self,
        lookup: &VerdictLookup<'_>,
        trusts: impl Fn(&Map<String, Value>) -> bool,
    ) -> Option<Footprint> {
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        load_candidates(&conn, lookup.hash, lookup.epoch)
            .iter()
            .find_map(|candidate| evaluate_candidate(&conn, candidate, lookup, &trusts))
    }

    /// Record a successful whole-lockfile verification under its private-route
    /// [`Footprint`] and merged policy snapshot. Best-effort: a DB error is
    /// swallowed (the next install just re-verifies). Callers must record only
    /// *passes*.
    pub(crate) fn record(&self, entry: VerdictRecord<'_>) {
        let footprint_key = entry.footprint.digest(entry.secret).unwrap_or_default();
        let Ok(footprint_json) = serde_json::to_string(entry.footprint) else {
            return;
        };
        let policy_json = Value::Object(entry.policy).to_string();
        let now_ms = now_ms();
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = conn.execute(
            "INSERT INTO lockfile_verdicts (
                 hash, footprint_key, footprint, policy, epoch, verified_at_ms
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(hash, epoch, footprint_key) DO UPDATE SET
                 footprint = excluded.footprint,
                 policy = excluded.policy,
                 verified_at_ms = excluded.verified_at_ms",
            rusqlite::params![
                entry.hash,
                footprint_key,
                footprint_json,
                policy_json,
                entry.epoch,
                now_ms
            ],
        );
        evict_hash_overflow(&conn, entry.hash, entry.epoch);
        evict_overflow(&conn);
    }
}

fn load_candidates(conn: &Connection, hash: &str, epoch: &str) -> Vec<StoredVerdict> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT footprint_key, footprint, policy
         FROM lockfile_verdicts
         WHERE hash = ?1 AND epoch = ?2
         ORDER BY verified_at_ms DESC",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map(rusqlite::params![hash, epoch], |row| {
        Ok(StoredVerdict {
            footprint_key: row.get(0)?,
            footprint_json: row.get(1)?,
            policy_json: row.get(2)?,
        })
    }) else {
        return Vec::new();
    };
    rows.flatten().collect()
}

fn evaluate_candidate(
    conn: &Connection,
    candidate: &StoredVerdict,
    lookup: &VerdictLookup<'_>,
    trusts: &impl Fn(&Map<String, Value>) -> bool,
) -> Option<Footprint> {
    let Ok(policy) = serde_json::from_str::<Map<String, Value>>(&candidate.policy_json) else {
        delete_candidate(conn, lookup.hash, lookup.epoch, &candidate.footprint_key);
        return None;
    };
    let Ok(footprint) = serde_json::from_str::<Footprint>(&candidate.footprint_json) else {
        delete_candidate(conn, lookup.hash, lookup.epoch, &candidate.footprint_key);
        return None;
    };
    if footprint.digest(lookup.secret).unwrap_or_default() != candidate.footprint_key {
        delete_candidate(conn, lookup.hash, lookup.epoch, &candidate.footprint_key);
        return None;
    }
    let caller_allowed =
        footprint.is_public() || footprint.allows(lookup.route_context, lookup.identity);
    (caller_allowed && trusts(&policy)).then_some(footprint)
}

fn delete_candidate(conn: &Connection, hash: &str, epoch: &str, footprint_key: &str) {
    let _ = conn.execute(
        "DELETE FROM lockfile_verdicts
         WHERE hash = ?1 AND epoch = ?2 AND footprint_key = ?3",
        rusqlite::params![hash, epoch, footprint_key],
    );
}

fn evict_hash_overflow(conn: &Connection, hash: &str, epoch: &str) {
    let _ = conn.execute(
        "DELETE FROM lockfile_verdicts WHERE rowid IN (
             SELECT rowid FROM lockfile_verdicts
             WHERE hash = ?1 AND epoch = ?2
             ORDER BY verified_at_ms DESC, rowid DESC
             LIMIT -1 OFFSET ?3
         )",
        rusqlite::params![hash, epoch, MAX_CANDIDATES_PER_HASH],
    );
}

/// Trim the oldest rows past [`MAX_ROWS`], ordered by `verified_at_ms`
/// (last-verification time, keeping [`VerdictCache::verified_footprint`] a pure
/// read on hits).
fn evict_overflow(conn: &Connection) {
    let _ = conn.execute(
        "DELETE FROM lockfile_verdicts WHERE rowid IN (
             SELECT rowid FROM lockfile_verdicts
             ORDER BY verified_at_ms DESC, rowid DESC
             LIMIT -1 OFFSET ?1
         )",
        rusqlite::params![MAX_ROWS],
    );
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
