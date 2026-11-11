use super::{CacheGcEffect, Duration, HashSet, Path, PathBuf, SystemTime};
use pnpm_store_dir::{StoreDir, StoreIndex, decode_package_files_index};
use rusqlite::{Connection, OpenFlags};
use std::time::UNIX_EPOCH;

const SQLITE_BUSY_WAIT: Duration = Duration::from_secs(5);
const VERDICT_TRIM_BATCH: i64 = 500;

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
) -> u64 {
    let db_path = verdict_db_path(cache_dir);
    tokio::task::spawn_blocking(move || expire_verdict_rows_sync(&db_path, max_age, effect))
        .await
        .unwrap_or(0)
}

fn expire_verdict_rows_sync(db_path: &Path, max_age: Duration, effect: CacheGcEffect) -> u64 {
    if !db_path.is_file() {
        return 0;
    }
    let ttl_ms = i64::try_from(max_age.as_millis()).unwrap_or(i64::MAX);
    let cutoff_ms = now_ms().saturating_sub(ttl_ms);
    match effect {
        CacheGcEffect::DryRun => estimate_expired_verdict_bytes(db_path, cutoff_ms),
        CacheGcEffect::Apply => delete_expired_verdict_rows(db_path, cutoff_ms),
    }
}

fn estimate_expired_verdict_bytes(db_path: &Path, cutoff_ms: i64) -> u64 {
    let Ok(conn) = open_sqlite(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
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
    effect: CacheGcEffect,
) -> u64 {
    let db_path = verdict_db_path(cache_dir);
    tokio::task::spawn_blocking(move || trim_verdict_db_sync(&db_path, excess_bytes, effect))
        .await
        .unwrap_or(0)
}

fn trim_verdict_db_sync(db_path: &Path, excess_bytes: u64, effect: CacheGcEffect) -> u64 {
    if excess_bytes == 0 || !db_path.is_file() {
        return 0;
    }
    match effect {
        CacheGcEffect::DryRun => estimate_oldest_verdict_bytes(db_path, excess_bytes),
        CacheGcEffect::Apply => delete_oldest_verdict_rows(db_path, excess_bytes),
    }
}

fn estimate_oldest_verdict_bytes(db_path: &Path, excess_bytes: u64) -> u64 {
    let Ok(conn) = open_sqlite(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return 0;
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT LENGTH(hash) + LENGTH(policy) + 16
         FROM lockfile_verdicts ORDER BY verified_at_ms ASC",
    ) else {
        return 0;
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, i64>(0)) else {
        return 0;
    };
    let mut reclaimed = 0u64;
    for row in rows.flatten() {
        reclaimed += u64::try_from(row).unwrap_or(0);
        if reclaimed >= excess_bytes {
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
    if excess_bytes >= before {
        let _ = conn.execute("DELETE FROM lockfile_verdicts", []);
        compact_sqlite(&conn);
        drop(conn);
        return before.saturating_sub(sqlite_family_bytes(db_path));
    }
    trim_verdict_batches(&conn, db_path, before, excess_bytes);
    drop(conn);
    before.saturating_sub(sqlite_family_bytes(db_path))
}

fn trim_verdict_batches(conn: &Connection, db_path: &Path, before: u64, excess_bytes: u64) {
    loop {
        let deleted = conn
            .execute(
                "DELETE FROM lockfile_verdicts WHERE hash IN (
                     SELECT hash FROM lockfile_verdicts
                     ORDER BY verified_at_ms ASC LIMIT ?1
                 )",
                [VERDICT_TRIM_BATCH],
            )
            .unwrap_or(0);
        if deleted == 0 {
            break;
        }
        compact_sqlite(conn);
        if before.saturating_sub(sqlite_family_bytes(db_path)) >= excess_bytes {
            break;
        }
    }
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

fn compact_sqlite(conn: &Connection) {
    let _ = conn.execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);");
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
}
