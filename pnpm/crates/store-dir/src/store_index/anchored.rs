#[cfg(target_os = "wasi")]
use super::wasi_lease;
use super::{
    Backend, INDEX_DIR, StoreIndex, StoreIndexError,
    files::{ALIAS_DIR, FilesIndex},
    root_mtime,
};
use pnpm_fs::OpenDir;
use rusqlite::{Connection, OpenFlags};
use std::{
    ffi::OsStr,
    io::{self, ErrorKind},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

const SQLITE_SIDECARS: [&str; 4] = ["index.db", "index.db-wal", "index.db-shm", "index.db-journal"];

impl StoreIndex {
    /// Open an existing store index anchored to `store_root` without creating
    /// directories or database files and without following symlinks.
    ///
    /// Returns `Ok(None)` when neither `index/` nor `index.db` exists yet, and
    /// `Err` if any index directory, database file, or SQLite sidecar is a
    /// symbolic link or cannot be anchored.
    pub fn open_existing_at(store_root: &OpenDir) -> Result<Option<Self>, StoreIndexError> {
        ensure_no_symlinks(store_root, &[INDEX_DIR, ALIAS_DIR])?;
        if store_root
            .open_child_dir(OsStr::new(INDEX_DIR))
            .map_err(|source| read_err(store_root, source))?
            .is_some()
        {
            return open_anchored_files_index(store_root).map(Some);
        }
        open_anchored_sqlite_index(store_root)
    }
}

fn open_anchored_files_index(store_root: &OpenDir) -> Result<StoreIndex, StoreIndexError> {
    let (anchor, anchored_root) = reopen_anchored(store_root)?;
    Ok(StoreIndex {
        backend: Backend::Files(FilesIndex::anchored(
            anchored_root.join(INDEX_DIR),
            Arc::new(anchor),
        )),
    })
}

fn open_anchored_sqlite_index(store_root: &OpenDir) -> Result<Option<StoreIndex>, StoreIndexError> {
    ensure_no_symlinks(store_root, &SQLITE_SIDECARS)?;
    if store_root
        .child_file_stat(OsStr::new("index.db"))
        .map_err(|source| read_err(store_root, source))?
        .is_none()
    {
        return Ok(None);
    }
    let (anchor, anchored_dir) = reopen_anchored(store_root)?;
    let db_path = anchored_dir.join("index.db");
    let root_mtime = root_mtime::acquire(&anchored_dir);
    #[cfg(target_os = "wasi")]
    let lease = wasi_lease::acquire(&db_path, true)?;
    let conn = open_nofollow_sqlite(&db_path)?;
    Ok(Some(StoreIndex::sqlite_with_anchor(
        conn,
        #[cfg(target_os = "wasi")]
        lease,
        Some(root_mtime),
        Some(anchor),
    )))
}

fn open_nofollow_sqlite(db_path: &std::path::Path) -> Result<Connection, StoreIndexError> {
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(|source| StoreIndexError::Open { path: db_path.to_path_buf(), source })?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|source| StoreIndexError::Open { path: db_path.to_path_buf(), source })?;
    Ok(conn)
}

fn ensure_no_symlinks(store_root: &OpenDir, names: &[&str]) -> Result<(), StoreIndexError> {
    for name in names {
        if store_root
            .has_symlink_child(OsStr::new(name))
            .map_err(|source| read_err(store_root, source))?
        {
            return Err(symlink_rejected(store_root, name));
        }
    }
    Ok(())
}

fn reopen_anchored(store_root: &OpenDir) -> Result<(OpenDir, PathBuf), StoreIndexError> {
    let anchor = store_root.try_reopen().map_err(|source| read_err(store_root, source))?;
    let anchored_dir = anchor
        .canonical_path()
        .map_err(|source| read_err(store_root, source))?
        .ok_or_else(|| symlink_rejected(store_root, "."))?;
    Ok((anchor, anchored_dir))
}

fn read_err(store_root: &OpenDir, source: io::Error) -> StoreIndexError {
    StoreIndexError::ReadEntry { path: store_root.path().to_path_buf(), source }
}

fn symlink_rejected(store_root: &OpenDir, name: &str) -> StoreIndexError {
    StoreIndexError::ReadEntry {
        path: store_root.path().join(name),
        source: io::Error::new(ErrorKind::InvalidInput, "refusing symlinked store index entry"),
    }
}
