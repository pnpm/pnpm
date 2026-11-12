mod conn;

use self::conn::{
    AnchoredVerdictDb, SQLITE_PAGE_BYTES, compact_sqlite, freelist_bytes, open_sqlite,
    open_sqlite_readonly, open_verdict_dir, pragma_u64, should_vacuum, should_vacuum_counts,
    verdict_db_and_wal_bytes, verdict_family_bytes, verdict_wal_bytes,
};
use super::{
    CacheGcEffect, Duration, Path, PathBuf, SystemTime, store_index::store_index_family_bytes,
};
use rusqlite::{Connection, OpenFlags};
use std::time::UNIX_EPOCH;

const EMPTY_VERDICT_DB_BYTES: u64 = 12 * 1024;

pub(super) struct VerdictExpiry {
    pub(super) reclaimed_bytes: u64,
    pub(super) deferred_free_bytes: u64,
    pub(super) cutoff_ms: i64,
}

pub(super) async fn resolver_sqlite_bytes(cache_dir: &Path) -> u64 {
    let root = cache_dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let verdict =
            open_verdict_dir(&root).map(|verdict| verdict_family_bytes(&verdict.dir)).unwrap_or(0);
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

struct VerdictPageMetrics {
    page_size: u64,
    page_count: u64,
    freelist: u64,
    ckpt_savings: u64,
    max_reclaimable: u64,
    live_reclaimable: u64,
}

fn read_page_metrics(
    conn: &Connection,
    db_and_wal: u64,
    extra_free_bytes: u64,
) -> VerdictPageMetrics {
    let page_size = pragma_u64(conn, "page_size").max(SQLITE_PAGE_BYTES);
    let page_count = pragma_u64(conn, "page_count");
    let freelist = pragma_u64(conn, "freelist_count");
    let post_ckpt_db_bytes = page_count.saturating_mul(page_size);
    let free_bytes = freelist.saturating_mul(page_size).saturating_add(extra_free_bytes);
    VerdictPageMetrics {
        page_size,
        page_count,
        freelist,
        ckpt_savings: db_and_wal.saturating_sub(post_ckpt_db_bytes),
        max_reclaimable: db_and_wal.saturating_sub(EMPTY_VERDICT_DB_BYTES),
        live_reclaimable: post_ckpt_db_bytes
            .saturating_sub(EMPTY_VERDICT_DB_BYTES)
            .saturating_sub(free_bytes),
    }
}

fn estimate_expired_verdict_bytes(verdict: &AnchoredVerdictDb, cutoff_ms: i64) -> (u64, u64) {
    let db_and_wal = verdict_db_and_wal_bytes(&verdict.dir);
    let wal_bytes = verdict_wal_bytes(&verdict.dir);
    let Ok(conn) = open_sqlite_readonly(&verdict.db_path, wal_bytes) else {
        return (0, 0);
    };
    let (expired_weight, total_weight) = query_verdict_weights(&conn, cutoff_ms);
    if expired_weight == 0 || total_weight == 0 {
        return (0, 0);
    }
    let metrics = read_page_metrics(&conn, db_and_wal, 0);
    let est_new_pages = proportional_bytes(metrics.live_reclaimable, expired_weight, total_weight)
        / metrics.page_size;
    let post_freelist = metrics.freelist.saturating_add(est_new_pages);
    if should_vacuum_counts(post_freelist, metrics.page_count) {
        let post_bytes = post_freelist.saturating_mul(metrics.page_size);
        (post_bytes.saturating_add(metrics.ckpt_savings).min(metrics.max_reclaimable), 0)
    } else {
        (metrics.ckpt_savings, est_new_pages.saturating_mul(metrics.page_size))
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
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
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
    let db_and_wal = verdict_db_and_wal_bytes(&verdict.dir);
    let wal_bytes = verdict_wal_bytes(&verdict.dir);
    let Ok(conn) = open_sqlite_readonly(&verdict.db_path, wal_bytes) else {
        return 0;
    };
    let metrics = read_page_metrics(&conn, db_and_wal, params.deferred_free_bytes);
    if metrics.max_reclaimable == 0 {
        return metrics.ckpt_savings;
    }
    let free_bytes = metrics.freelist
        .saturating_mul(metrics.page_size)
        .saturating_add(params.deferred_free_bytes);
    let non_row_savings = metrics.ckpt_savings.saturating_add(free_bytes);
    let needed_from_rows = params.excess_bytes.saturating_sub(non_row_savings);
    if needed_from_rows == 0 {
        return non_row_savings.min(metrics.max_reclaimable);
    }
    let floor = params.min_verified_at_ms.unwrap_or(i64::MIN);
    let row_bytes = estimate_trimmed_row_bytes(
        &conn,
        metrics.live_reclaimable,
        needed_from_rows,
        floor,
        metrics.page_size,
    );
    non_row_savings.saturating_add(row_bytes).min(metrics.max_reclaimable)
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
    let target_bytes = before.saturating_sub(excess_bytes);
    let Ok(conn) = open_sqlite(&verdict.db_path, OpenFlags::SQLITE_OPEN_READ_WRITE) else {
        return 0;
    };
    if verdict_wal_bytes(&verdict.dir) > 0 {
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }
    let current = verdict_family_bytes(&verdict.dir);
    if current > target_bytes {
        trim_and_compact_verdict_rows(&conn, current - target_bytes);
    }
    drop(conn);
    before.saturating_sub(verdict_family_bytes(&verdict.dir))
}

fn trim_and_compact_verdict_rows(conn: &Connection, remaining_excess: u64) {
    let free_bytes = freelist_bytes(conn);
    let page_size = pragma_u64(conn, "page_size").max(SQLITE_PAGE_BYTES);
    let db_bytes = pragma_u64(conn, "page_count").saturating_mul(page_size);
    let needed_from_rows = remaining_excess.saturating_sub(free_bytes);
    let live_reclaimable =
        db_bytes.saturating_sub(EMPTY_VERDICT_DB_BYTES).saturating_sub(free_bytes);
    let deleted = execute_verdict_trim(conn, live_reclaimable, needed_from_rows);
    if deleted > 0 || free_bytes > 0 {
        compact_sqlite(conn);
    }
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

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
