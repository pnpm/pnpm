use super::{Duration, Path, PathBuf};
use pnpm_fs::OpenDir;
use rusqlite::{Connection, OpenFlags};
use std::ffi::OsStr;

const SQLITE_BUSY_WAIT: Duration = Duration::from_secs(5);
pub(super) const SQLITE_PAGE_BYTES: u64 = 4096;
const VERDICT_DIR: &str = "resolver-cache";
const VERDICT_DB: &str = "lockfile-verdicts.sqlite";
const VERDICT_WAL: &str = "lockfile-verdicts.sqlite-wal";
const VERDICT_SHM: &str = "lockfile-verdicts.sqlite-shm";
const VERDICT_FAMILY: [&str; 4] =
    [VERDICT_DB, VERDICT_WAL, VERDICT_SHM, "lockfile-verdicts.sqlite-journal"];

pub(super) struct AnchoredVerdictDb {
    pub(super) dir: OpenDir,
    pub(super) db_path: PathBuf,
}

pub(super) fn open_verdict_dir(cache_root: &OpenDir) -> Option<AnchoredVerdictDb> {
    if cache_root
        .has_symlink_child(OsStr::new(VERDICT_DIR))
        .ok()?
    {
        return None;
    }
    let dir = cache_root
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

pub(super) fn verdict_family_bytes_in_cache(cache_root: &OpenDir) -> std::io::Result<u64> {
    let Some(dir) = cache_root.open_child_dir(OsStr::new(VERDICT_DIR))? else {
        return Ok(0);
    };
    let mut total = 0u64;
    for name in VERDICT_FAMILY {
        if let Some(stat) = dir.child_file_stat(OsStr::new(name))? {
            total = total.saturating_add(stat.len);
        }
    }
    Ok(total)
}

pub(super) fn verdict_family_bytes(dir: &OpenDir) -> u64 {
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

pub(super) fn verdict_wal_bytes(dir: &OpenDir) -> u64 {
    child_len(dir, VERDICT_WAL)
}

pub(super) fn verdict_db_and_wal_bytes(dir: &OpenDir) -> u64 {
    child_len(dir, VERDICT_DB) + child_len(dir, VERDICT_WAL)
}

fn child_len(dir: &OpenDir, name: &str) -> u64 {
    dir.child_file_stat(OsStr::new(name))
        .ok()
        .flatten()
        .map(|stat| stat.len)
        .unwrap_or(0)
}

pub(super) fn open_sqlite(db_path: &Path, flags: OpenFlags) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(db_path, flags | OpenFlags::SQLITE_OPEN_NOFOLLOW)?;
    conn.busy_timeout(SQLITE_BUSY_WAIT)?;
    Ok(conn)
}

pub(super) fn open_verdict_readonly(verdict: &AnchoredVerdictDb) -> rusqlite::Result<Connection> {
    let wal_bytes = verdict_wal_bytes(&verdict.dir);
    let has_shm = verdict.dir
        .child_file_stat(OsStr::new(VERDICT_SHM))
        .ok()
        .flatten()
        .is_some();
    if wal_bytes == 0
        && !has_shm
        && let Ok(conn) = open_sqlite(&verdict.db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
    {
        conn.pragma_update(None, "query_only", true)?;
        return Ok(conn);
    }
    open_sqlite(&verdict.db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
}

pub(super) fn should_vacuum(conn: &Connection) -> bool {
    should_vacuum_counts(pragma_u64(conn, "freelist_count"), pragma_u64(conn, "page_count"))
}

pub(super) fn should_vacuum_counts(freelist: u64, pages: u64) -> bool {
    freelist >= 16 && freelist.saturating_mul(4) >= pages
}

pub(super) fn page_size_bytes(conn: &Connection) -> u64 {
    match pragma_u64(conn, "page_size") {
        0 => SQLITE_PAGE_BYTES,
        size => size,
    }
}

pub(super) fn freelist_bytes(conn: &Connection) -> u64 {
    let freelist = pragma_u64(conn, "freelist_count");
    freelist.saturating_mul(page_size_bytes(conn))
}

pub(super) fn pragma_u64(conn: &Connection, name: &str) -> u64 {
    conn.pragma_query_value(None, name, |row| row.get::<_, i64>(0))
        .ok()
        .and_then(|val| u64::try_from(val).ok())
        .unwrap_or(0)
}

pub(super) fn compact_sqlite(conn: &Connection) {
    let _ = conn.execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);");
}
