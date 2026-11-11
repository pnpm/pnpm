use super::{CacheGcEffect, Duration, HashSet, Path, PathBuf, SystemTime};
use pnpm_store_dir::{StoreDir, StoreIndex, decode_package_files_index};
use rusqlite::{Connection, OpenFlags};
use std::time::UNIX_EPOCH;

const SQLITE_BUSY_WAIT: Duration = Duration::from_secs(5);
const EMPTY_VERDICT_DB_BYTES: u64 = 12 * 1024;
const SQLITE_PAGE_BYTES: u64 = 4096;

pub(super) struct VerdictExpiry {
    pub(super) reclaimed_bytes: u64,
    pub(super) cutoff_ms: i64,
}

fn verdict_db_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("resolver-cache").join("lockfile-verdicts.sqlite")
}

fn store_index_db_path(cache_dir: &Path) -> PathBuf {
    StoreDir::new(cache_dir.join("resolver-store")).root().join("index.db")
}

fn sqlite_family_bytes(db_path: &Path) -> u64 {
    let Some(base_name) = db_path.file_name().and_then(|name| name.to_str()) else {
        return 0;
    };
    ["", "-wal", "-shm", "-journal"]
        .into_iter()
        .filter_map(|suffix| {
            std::fs::metadata(db_path.with_file_name(format!("{base_name}{suffix}"))).ok()
        })
        .map(|meta| meta.len())
        .sum()
}

pub(super) async fn resolver_sqlite_bytes(cache_dir: &Path) -> u64 {
    let root = cache_dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        sqlite_family_bytes(&verdict_db_path(&root))
            + sqlite_family_bytes(&store_index_db_path(&root))
    })
    .await
    .unwrap_or(0)
}

pub(super) async fn expire_verdict_rows(
    cache_dir: &Path,
    max_age: Duration,
    effect: CacheGcEffect,
) -> VerdictExpiry {
    let db_path = verdict_db_path(cache_dir);
    tokio::task::spawn_blocking(move || expire_verdict_rows_sync(&db_path, max_age, effect))
        .await
        .unwrap_or(VerdictExpiry { reclaimed_bytes: 0, cutoff_ms: 0 })
}

fn expire_verdict_rows_sync(
    db_path: &Path,
    max_age: Duration,
    effect: CacheGcEffect,
) -> VerdictExpiry {
    let ttl_ms = i64::try_from(max_age.as_millis()).unwrap_or(i64::MAX);
    let cutoff_ms = now_ms().saturating_sub(ttl_ms);
    if !db_path.is_file() {
        return VerdictExpiry { reclaimed_bytes: 0, cutoff_ms };
    }
    let reclaimed_bytes = match effect {
        CacheGcEffect::DryRun => estimate_expired_verdict_bytes(db_path, cutoff_ms),
        CacheGcEffect::Apply => delete_expired_verdict_rows(db_path, cutoff_ms),
    };
    VerdictExpiry { reclaimed_bytes, cutoff_ms }
}

fn estimate_expired_verdict_bytes(db_path: &Path, cutoff_ms: i64) -> u64 {
    let Ok(conn) = open_sqlite_readonly(db_path) else {
        return 0;
    };
    let bytes: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(LENGTH(hash) + LENGTH(policy) + 16), 0)
             FROM lockfile_verdicts WHERE verified_at_ms <= ?1",
            [cutoff_ms],
            |row| row.get(0),
        )
        .unwrap_or(0);
    u64::try_from(bytes).unwrap_or(0)
}

fn delete_expired_verdict_rows(db_path: &Path, cutoff_ms: i64) -> u64 {
    let before = sqlite_family_bytes(db_path);
    let Ok(conn) = open_sqlite(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE) else {
        return 0;
    };
    let deleted = conn
        .execute("DELETE FROM lockfile_verdicts WHERE verified_at_ms <= ?1", [cutoff_ms])
        .unwrap_or(0);
    if deleted == 0 {
        return 0;
    }
    compact_sqlite(&conn);
    drop(conn);
    before.saturating_sub(sqlite_family_bytes(db_path))
}

pub(super) async fn trim_verdict_db_to_fit(
    cache_dir: &Path,
    excess_bytes: u64,
    min_verified_at_ms: Option<i64>,
    effect: CacheGcEffect,
) -> u64 {
    let db_path = verdict_db_path(cache_dir);
    tokio::task::spawn_blocking(move || {
        trim_verdict_db_sync(&db_path, excess_bytes, min_verified_at_ms, effect)
    })
    .await
    .unwrap_or(0)
}

fn trim_verdict_db_sync(
    db_path: &Path,
    excess_bytes: u64,
    min_verified_at_ms: Option<i64>,
    effect: CacheGcEffect,
) -> u64 {
    if excess_bytes == 0 || !db_path.is_file() {
        return 0;
    }
    match effect {
        CacheGcEffect::DryRun => {
            estimate_oldest_verdict_bytes(db_path, excess_bytes, min_verified_at_ms)
        }
        CacheGcEffect::Apply => delete_oldest_verdict_rows(db_path, excess_bytes),
    }
}

fn estimate_oldest_verdict_bytes(
    db_path: &Path,
    excess_bytes: u64,
    min_verified_at_ms: Option<i64>,
) -> u64 {
    let Ok(conn) = open_sqlite_readonly(db_path) else {
        return 0;
    };
    let floor = min_verified_at_ms.unwrap_or(i64::MIN);
    let Ok(mut stmt) = conn.prepare(
        "SELECT LENGTH(hash) + LENGTH(policy) + 16
         FROM lockfile_verdicts
         WHERE verified_at_ms > ?1
         ORDER BY verified_at_ms ASC, rowid ASC",
    ) else {
        return 0;
    };
    let Ok(rows) = stmt.query_map([floor], |row| row.get::<_, i64>(0)) else {
        return 0;
    };
    sum_until_target(rows.flatten(), excess_bytes)
}

fn sum_until_target(rows: impl Iterator<Item = i64>, target: u64) -> u64 {
    let mut reclaimed = 0u64;
    for row in rows {
        reclaimed = reclaimed.saturating_add(u64::try_from(row).unwrap_or(0));
        if reclaimed >= target {
            break;
        }
    }
    reclaimed
}

fn delete_oldest_verdict_rows(db_path: &Path, excess_bytes: u64) -> u64 {
    let before = sqlite_family_bytes(db_path);
    let Ok(conn) = open_sqlite(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE) else {
        return 0;
    };
    let reclaimable = before.saturating_sub(EMPTY_VERDICT_DB_BYTES);
    if excess_bytes >= reclaimable {
        let _ = conn.execute("DELETE FROM lockfile_verdicts", []);
    } else if let Some(limit) = rows_to_delete(&conn, reclaimable, excess_bytes) {
        let _ = conn.execute(
            "DELETE FROM lockfile_verdicts WHERE rowid IN (
                 SELECT rowid FROM lockfile_verdicts
                 ORDER BY verified_at_ms ASC, rowid ASC LIMIT ?1
             )",
            [limit],
        );
    }
    compact_sqlite(&conn);
    drop(conn);
    before.saturating_sub(sqlite_family_bytes(db_path))
}

fn rows_to_delete(conn: &Connection, reclaimable: u64, excess_bytes: u64) -> Option<i64> {
    let mut stmt = conn
        .prepare(
            "SELECT 2 * LENGTH(hash) + LENGTH(policy) + 24
             FROM lockfile_verdicts ORDER BY verified_at_ms ASC, rowid ASC",
        )
        .ok()?;
    let weights: Vec<u64> = stmt
        .query_map([], |row| row.get::<_, i64>(0))
        .ok()?
        .flatten()
        .map(|val| u64::try_from(val).unwrap_or(0))
        .collect();
    let total_weight: u64 = weights.iter().sum();
    if total_weight == 0 || reclaimable == 0 {
        return None;
    }
    let target_bytes = excess_bytes.saturating_add(SQLITE_PAGE_BYTES).min(reclaimable);
    let target_weight =
        (u128::from(total_weight) * u128::from(target_bytes)).div_ceil(u128::from(reclaimable));
    let mut cumulative = 0u128;
    let mut count = 0i64;
    for weight in weights {
        count += 1;
        cumulative += u128::from(weight);
        if cumulative >= target_weight {
            break;
        }
    }
    (count > 0).then_some(count)
}

pub(super) async fn prune_store_index_digests(
    cache_dir: &Path,
    removed_digests: HashSet<String>,
) -> u64 {
    if removed_digests.is_empty() {
        return 0;
    }
    let root = cache_dir.to_path_buf();
    tokio::task::spawn_blocking(move || prune_store_index_sync(&root, &removed_digests))
        .await
        .unwrap_or(0)
}

fn prune_store_index_sync(cache_dir: &Path, removed_digests: &HashSet<String>) -> u64 {
    let store_dir = StoreDir::new(cache_dir.join("resolver-store"));
    let db_path = store_dir.root().join("index.db");
    if !db_path.is_file() {
        return 0;
    }
    let before = sqlite_family_bytes(&db_path);
    let Ok(mut index) = StoreIndex::open_in(&store_dir) else {
        return 0;
    };
    let mut keys = Vec::new();
    let _ = index.for_each_raw(|key, data| {
        if let Ok(pkg) = decode_package_files_index(&data)
            && pkg.files
                .values()
                .any(|file| removed_digests.contains(&file.digest))
        {
            keys.push(key);
        }
        Ok::<(), pnpm_store_dir::StoreIndexError>(())
    });
    let _ = index.delete_many(&keys);
    drop(index);
    before.saturating_sub(sqlite_family_bytes(&db_path))
}

fn open_sqlite(db_path: &Path, flags: OpenFlags) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(db_path, flags)?;
    conn.busy_timeout(SQLITE_BUSY_WAIT)?;
    Ok(conn)
}

fn open_sqlite_readonly(db_path: &Path) -> rusqlite::Result<Connection> {
    if !wal_has_frames(db_path)
        && let Some(uri) = immutable_sqlite_uri(db_path)
    {
        return Connection::open_with_flags(
            uri,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        );
    }
    open_sqlite(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
}

fn wal_has_frames(db_path: &Path) -> bool {
    let Some(base_name) = db_path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    std::fs::metadata(db_path.with_file_name(format!("{base_name}-wal")))
        .is_ok_and(|meta| meta.len() > 0)
}

fn immutable_sqlite_uri(db_path: &Path) -> Option<String> {
    let absolute = std::path::absolute(db_path).ok()?;
    let mut url = url::Url::from_file_path(&absolute).ok()?;
    url.query_pairs_mut().append_pair("immutable", "1");
    Some(url.into())
}

fn compact_sqlite(conn: &Connection) {
    let _ = conn.execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);");
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
