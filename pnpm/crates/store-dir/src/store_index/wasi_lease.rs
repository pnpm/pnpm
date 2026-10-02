use std::{
    collections::HashMap,
    fs::File,
    io,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex, Weak},
};

use super::StoreIndexError;

/// Keeps every connection in this process behind one process-lifetime lease.
/// SQLite's dotfile VFS cannot recover a dead owner's lock by itself.
pub(super) struct IndexLease {
    _file: File,
    database: PathBuf,
}

pub(super) fn acquire(database: &Path, recover: bool) -> Result<Arc<IndexLease>, StoreIndexError> {
    acquire_inner(database, recover)
        .map_err(|source| StoreIndexError::Lease { path: database.to_path_buf(), source })
}

fn acquire_inner(database: &Path, recover: bool) -> io::Result<Arc<IndexLease>> {
    // SAFETY: sqlite3_threadsafe returns a compile-time constant and takes no pointers.
    if unsafe { rusqlite::ffi::sqlite3_threadsafe() } == 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "WASI SQLite requires SQLITE_THREADSAFE=1",
        ));
    }
    static LEASES: LazyLock<Mutex<HashMap<PathBuf, Weak<IndexLease>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let database = pnpm_fs::realpath_missing(&std::path::absolute(database)?)?;
    pnpm_fs::check_file_owner(&pnpm_fs::open_file_without_following(&database)?)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::PermissionDenied {
                io::Error::new(
                    error.kind(),
                    "WebContainer SQLite stores must belong to the current user",
                )
            } else {
                error
            }
        })?;
    let mut leases = LEASES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(lease) = leases.get(&database).and_then(Weak::upgrade) {
        return Ok(lease);
    }
    let path = pnpm_fs::secure_user_lock_file_path("pnpm-wasm-sqlite", &database, "lock")?;
    let file = pnpm_fs::open_secure_lock_file(&path)?;
    pnpm_fs::lock_file(&file, true)?;
    if recover {
        remove_abandoned_dotfile(&database)?;
    }
    pnpm_fs::register_sqlite_permissions(&database, true)?;
    let lease = Arc::new(IndexLease { _file: file, database: database.clone() });
    leases.insert(database, Arc::downgrade(&lease));
    Ok(lease)
}

fn remove_abandoned_dotfile(database: &Path) -> io::Result<()> {
    let mut lock = database.as_os_str().to_os_string();
    lock.push(".lock");
    match std::fs::remove_dir(PathBuf::from(lock)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

impl Drop for IndexLease {
    fn drop(&mut self) {
        // All SQLite connections close before the final lease is dropped.
        if let Err(error) = pnpm_fs::register_sqlite_permissions(&self.database, false) {
            tracing::warn!(path = %self.database.display(), %error, "Failed to unregister SQLite sidecar permissions");
        }
    }
}
