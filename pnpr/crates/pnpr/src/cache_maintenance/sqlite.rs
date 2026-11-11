use super::{
    CacheGcEffect, Duration, Path, PathBuf, SystemTime, store_index::store_index_family_bytes,
};
use pnpm_fs::OpenDir;
use rusqlite::{Connection, OpenFlags};
use std::{ffi::OsStr, time::UNIX_EPOCH};

const SQLITE_BUSY_WAIT: Duration = Duration::from_secs(5);
const EMPTY_VERDICT_DB_BYTES: u64 = 12 * 1024;
const SQLITE_PAGE_BYTES: u64 = 4096;
const VERDICT_DIR: &str = "resolver-cache";
const VERDICT_DB: &str = "lockfile-verdicts.sqlite";
const VERDICT_WAL: &str = "lockfile-verdicts.sqlite-wal";
const VERDICT_FAMILY: [&str; 4] =
    [VERDICT_DB, VERDICT_WAL, "lockfile-verdicts.sqlite-shm", "lockfile-verdicts.sqlite-journal"];

pub(super) struct VerdictExpiry {
    pub(super) reclaimed_bytes: u64,
    pub(super) deferred_free_bytes: u64,
    pub(super) cutoff_ms: i64,
}

struct AnchoredVerdictDb {
    dir: OpenDir,
    db_path: PathBuf,
}

fn open_verdict_dir(cache_dir: &Path) -> Option<AnchoredVerdictDb> {
    let root = OpenDir::open(cache_dir).ok()?;
    if root
        .has_symlink_child(OsStr::new(VERDICT_DIR))
        .ok()?
    {
        return None;
    }
    let dir = root
        .open_child_dir(OsStr::new(VERDICT_DIR))
        .ok()??;
    if VERDICT_FAMILY
        .iter()
        .any(|name| {
            dir.has_symlink_child(OsStr::new(name))
                .unwrap_or(true)
        })
    {
        return None;
    }
    dir.child_file_stat(OsStr::new(VERDICT_DB))
        .ok()??;
    let anchored_dir = dir.canonical_path().ok()??;
    Some(AnchoredVerdictDb { dir, db_path: anchored_dir.join(VERDICT_DB) })
}

fn verdict_family_bytes(dir: &OpenDir) -> u64 {
    VERDICT_FAMILY
        .iter()
        .filter_map(|name| {
            dir.child_file_stat(OsStr::new(name))
                .ok()
                .flatten()
        })
        .map(|stat| stat.len)
        .sum()
}

fn verdict_wal_bytes(dir: &OpenDir) -> u64 {
    dir.child_file_stat(OsStr::new(VERDICT_WAL))
        .ok()
        .flatten()
        .map_or(0, |stat| stat.len)
}

pub(super) async fn resolver_sqlite_bytes(cache_dir: &Path) -> u64 {
    let root = cache_dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let verdict =
            open_verdict_dir(&root).map_or(0, |verdict| verdict_family_bytes(&verdict.dir));
        verdict + store_index_family_bytes(&root)
    })
    .await
    .unwrap_or(0)
}

pub(super) async fn expire_verdict_rows(
    cache_dir: &Path,
    max_age: Duration,
    effect: CacheGcEffect,
) -> VerdictExpiry {
    let root = cache_dir.to_path_buf();
    tokio::task::spawn_blocking(move || expire_verdict_rows_sync(&root, max_age, effect))
        .await
        .unwrap_or(VerdictExpiry { reclaimed_bytes: 0, deferred_free_bytes: 0, cutoff_ms: 0 })
}

fn expire_verdict_rows_sync(
    cache_dir: &Path,
    max_age: Duration,
    effect: CacheGcEffect,
) -> VerdictExpiry {
    let ttl_ms = i64::try_from(max_age.as_millis()).unwrap_or(i64::MAX);
    let cutoff_ms = now_ms().saturating_sub(ttl_ms);
    let Some(verdict) = open_verdict_dir(cache_dir) else {
        return VerdictExpiry { reclaimed_bytes: 0, deferred_free_bytes: 0, cutoff_ms };
    };
    let (reclaimed_bytes, deferred_free_bytes) = match effect {
        CacheGcEffect::DryRun => estimate_expired_verdict_bytes(&verdict, cutoff_ms),
        CacheGcEffect::Apply => (delete_expired_verdict_rows(&verdict, cutoff_ms), 0),
    };
    VerdictExpiry { reclaimed_bytes, deferred_free_bytes, cutoff_ms }
}

fn estimate_expired_verdict_bytes(verdict: &AnchoredVerdictDb, cutoff_ms: i64) -> (u64, u64) {
    let wal_bytes = verdict_wal_bytes(&verdict.dir);
    let Ok(conn) = open_sqlite_readonly(&verdict.db_path, wal_bytes) else {
        return (0, 0);
    };
    let (expired_weight, total_weight) = query_verdict_weights(&conn, cutoff_ms);
    if expired_weight == 0 || total_weight == 0 {
        return (0, 0);
    }
    let page_size = pragma_u64(&conn, "page_size").max(SQLITE_PAGE_BYTES);
    let freelist = pragma_u64(&conn, "freelist_count");
    let max_reclaimable = verdict_family_bytes(&verdict.dir).saturating_sub(EMPTY_VERDICT_DB_BYTES);
    let live_reclaimable = max_reclaimable
        .saturating_sub(freelist.saturating_mul(page_size))
        .saturating_sub(wal_bytes);
    let est_new_pages =
        proportional_bytes(live_reclaimable, expired_weight, total_weight) / page_size;
    let post_freelist = freelist.saturating_add(est_new_pages);
    let post_bytes = post_freelist.saturating_mul(page_size);
    if should_vacuum_counts(post_freelist, pragma_u64(&conn, "page_count")) {
        (post_bytes.saturating_add(wal_bytes).min(max_reclaimable), 0)
    } else {
        (0, est_new_pages.saturating_mul(page_size))
    }
}

fn query_verdict_weights(conn: &Connection, cutoff_ms: i64) -> (u64, u64) {
    conn.query_row(
        "SELECT
             COALESCE(SUM(CASE WHEN verified_at_ms <= ?1 THEN 2 * LENGTH(hash) + LENGTH(policy) + 24 ELSE 0 END), 0),
             COALESCE(SUM(2 * LENGTH(hash) + LENGTH(policy) + 24), 0)
         FROM lockfile_verdicts",
        [cutoff_ms],
        |row| {
            let exp = u64::try_from(row.get::<_, i64>(0)?).unwrap_or(0);
            let tot = u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0);
            Ok((exp, tot))
        },
    )
    .unwrap_or((0, 0))
}

fn delete_expired_verdict_rows(verdict: &AnchoredVerdictDb, cutoff_ms: i64) -> u64 {
    let before = verdict_family_bytes(&verdict.dir);
    let Ok(conn) = open_sqlite(&verdict.db_path, OpenFlags::SQLITE_OPEN_READ_WRITE) else {
        return 0;
    };
    let deleted = conn
        .execute("DELETE FROM lockfile_verdicts WHERE verified_at_ms <= ?1", [cutoff_ms])
        .unwrap_or(0);
    if deleted == 0 {
        return 0;
    }
    if should_vacuum(&conn) {
        compact_sqlite(&conn);
    } else {
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);");
        return 0;
    }
    drop(conn);
    before.saturating_sub(verdict_family_bytes(&verdict.dir))
}

pub(super) struct VerdictTrimParams {
    pub(super) excess_bytes: u64,
    pub(super) min_verified_at_ms: Option<i64>,
    pub(super) deferred_free_bytes: u64,
    pub(super) effect: CacheGcEffect,
}

pub(super) async fn trim_verdict_db_to_fit(cache_dir: &Path, params: VerdictTrimParams) -> u64 {
    let root = cache_dir.to_path_buf();
    tokio::task::spawn_blocking(move || trim_verdict_db_sync(&root, &params))
        .await
        .unwrap_or(0)
}

fn trim_verdict_db_sync(cache_dir: &Path, params: &VerdictTrimParams) -> u64 {
    if params.excess_bytes == 0 {
        return 0;
    }
    let Some(verdict) = open_verdict_dir(cache_dir) else {
        return 0;
    };
    match params.effect {
        CacheGcEffect::DryRun => estimate_oldest_verdict_bytes(&verdict, params),
        CacheGcEffect::Apply => delete_oldest_verdict_rows(&verdict, params.excess_bytes),
    }
}

fn estimate_oldest_verdict_bytes(verdict: &AnchoredVerdictDb, params: &VerdictTrimParams) -> u64 {
    let before = verdict_family_bytes(&verdict.dir);
    let wal_bytes = verdict_wal_bytes(&verdict.dir);
    let max_reclaimable = before.saturating_sub(EMPTY_VERDICT_DB_BYTES);
    let Ok(conn) = open_sqlite_readonly(&verdict.db_path, wal_bytes) else {
        return 0;
    };
    let page_size = pragma_u64(&conn, "page_size").max(SQLITE_PAGE_BYTES);
    let free_bytes = freelist_bytes(&conn).saturating_add(params.deferred_free_bytes);
    let non_row_savings = free_bytes.saturating_add(wal_bytes);
    if max_reclaimable == 0 {
        return wal_bytes.min(before);
    }
    let needed_from_rows = params.excess_bytes.saturating_sub(non_row_savings);
    let live_reclaimable = max_reclaimable.saturating_sub(non_row_savings);
    if needed_from_rows == 0 {
        return non_row_savings.min(max_reclaimable);
    }
    if needed_from_rows >= live_reclaimable {
        return max_reclaimable;
    }
    let floor = params.min_verified_at_ms.unwrap_or(i64::MIN);
    let row_bytes =
        estimate_trimmed_row_bytes(&conn, live_reclaimable, needed_from_rows, floor, page_size);
    non_row_savings.saturating_add(row_bytes).min(max_reclaimable)
}

fn estimate_trimmed_row_bytes(
    conn: &Connection,
    reclaimable: u64,
    excess_bytes: u64,
    floor_ms: i64,
    page_size: u64,
) -> u64 {
    let Some(weights) = load_row_weights(conn, floor_ms) else {
        return 0;
    };
    let Some((count, total_weight)) = select_row_prefix(&weights, reclaimable, excess_bytes) else {
        return 0;
    };
    let selected_weight: u64 = weights.iter().take(count).sum();
    let raw = proportional_bytes(reclaimable, selected_weight, total_weight);
    (raw / page_size).saturating_mul(page_size)
}

fn delete_oldest_verdict_rows(verdict: &AnchoredVerdictDb, excess_bytes: u64) -> u64 {
    let before = verdict_family_bytes(&verdict.dir);
    let wal_bytes = verdict_wal_bytes(&verdict.dir);
    let Ok(conn) = open_sqlite(&verdict.db_path, OpenFlags::SQLITE_OPEN_READ_WRITE) else {
        return 0;
    };
    let free_bytes = freelist_bytes(&conn);
    let non_row_savings = free_bytes.saturating_add(wal_bytes);
    let needed_from_rows = excess_bytes.saturating_sub(non_row_savings);
    let live_reclaimable =
        before.saturating_sub(EMPTY_VERDICT_DB_BYTES).saturating_sub(non_row_savings);
    let deleted = execute_verdict_trim(&conn, live_reclaimable, needed_from_rows);
    if deleted > 0 || free_bytes > 0 {
        compact_sqlite(&conn);
    } else if wal_bytes > 0 {
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }
    drop(conn);
    before.saturating_sub(verdict_family_bytes(&verdict.dir))
}

fn execute_verdict_trim(conn: &Connection, live_reclaimable: u64, needed_from_rows: u64) -> usize {
    if needed_from_rows == 0 || live_reclaimable == 0 {
        return 0;
    }
    if needed_from_rows >= live_reclaimable {
        return conn
            .execute("DELETE FROM lockfile_verdicts", [])
            .unwrap_or(0);
    }
    let Some(weights) = load_row_weights(conn, i64::MIN) else {
        return 0;
    };
    let Some((count, _)) = select_row_prefix(&weights, live_reclaimable, needed_from_rows) else {
        return 0;
    };
    let limit = i64::try_from(count).unwrap_or(i64::MAX);
    conn.execute(
        "DELETE FROM lockfile_verdicts WHERE rowid IN (
             SELECT rowid FROM lockfile_verdicts
             ORDER BY verified_at_ms ASC, rowid ASC LIMIT ?1
         )",
        [limit],
    )
    .unwrap_or(0)
}

fn load_row_weights(conn: &Connection, floor_ms: i64) -> Option<Vec<u64>> {
    let mut stmt = conn
        .prepare(
            "SELECT 2 * LENGTH(hash) + LENGTH(policy) + 24
             FROM lockfile_verdicts
             WHERE verified_at_ms > ?1
             ORDER BY verified_at_ms ASC, rowid ASC",
        )
        .ok()?;
    let weights = stmt
        .query_map([floor_ms], |row| row.get::<_, i64>(0))
        .ok()?
        .flatten()
        .map(|val| u64::try_from(val).unwrap_or(0))
        .collect();
    Some(weights)
}

fn select_row_prefix(weights: &[u64], reclaimable: u64, excess_bytes: u64) -> Option<(usize, u64)> {
    let total_weight: u64 = weights.iter().sum();
    if total_weight == 0 || reclaimable == 0 {
        return None;
    }
    let target_bytes = excess_bytes.saturating_add(SQLITE_PAGE_BYTES).min(reclaimable);
    let target_weight =
        (u128::from(total_weight) * u128::from(target_bytes)).div_ceil(u128::from(reclaimable));
    let mut cumulative = 0u128;
    for (idx, &weight) in weights.iter().enumerate() {
        cumulative += u128::from(weight);
        if cumulative >= target_weight {
            return Some((idx + 1, total_weight));
        }
    }
    (!weights.is_empty()).then_some((weights.len(), total_weight))
}

fn proportional_bytes(total_bytes: u64, part_weight: u64, total_weight: u64) -> u64 {
    if total_weight == 0 {
        return 0;
    }
    u64::try_from((u128::from(total_bytes) * u128::from(part_weight)) / u128::from(total_weight))
        .unwrap_or(u64::MAX)
}

fn open_sqlite(db_path: &Path, flags: OpenFlags) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(db_path, flags | OpenFlags::SQLITE_OPEN_NOFOLLOW)?;
    conn.busy_timeout(SQLITE_BUSY_WAIT)?;
    Ok(conn)
}

fn open_sqlite_readonly(db_path: &Path, wal_bytes: u64) -> rusqlite::Result<Connection> {
    if wal_bytes == 0
        && let Some(uri) = immutable_sqlite_uri(db_path)
        && let Ok(conn) = Connection::open_with_flags(
            uri,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_URI
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
    {
        return Ok(conn);
    }
    open_sqlite(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
}

fn immutable_sqlite_uri(db_path: &Path) -> Option<String> {
    let absolute = std::path::absolute(db_path).ok()?;
    let mut url = url::Url::from_file_path(&absolute).ok()?;
    url.query_pairs_mut().append_pair("immutable", "1");
    Some(url.into())
}

fn should_vacuum(conn: &Connection) -> bool {
    should_vacuum_counts(pragma_u64(conn, "freelist_count"), pragma_u64(conn, "page_count"))
}

fn should_vacuum_counts(freelist: u64, pages: u64) -> bool {
    freelist >= 16 && freelist.saturating_mul(4) >= pages
}

fn freelist_bytes(conn: &Connection) -> u64 {
    let freelist = pragma_u64(conn, "freelist_count");
    let page_size = pragma_u64(conn, "page_size").max(SQLITE_PAGE_BYTES);
    freelist.saturating_mul(page_size)
}

fn pragma_u64(conn: &Connection, name: &str) -> u64 {
    conn.pragma_query_value(None, name, |row| row.get::<_, i64>(0))
        .ok()
        .and_then(|val| u64::try_from(val).ok())
        .unwrap_or(0)
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
