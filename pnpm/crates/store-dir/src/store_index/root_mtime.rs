use std::{
    collections::HashMap,
    ffi::OsString,
    fs::{self, File, FileTimes},
    io,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex, Weak},
    time::SystemTime,
};

/// Restores the store root's mtime once the last `SQLite` connection in this
/// process closes, if the root's entries are unchanged.
///
/// `SQLite` creates the `index.db-wal` and `index.db-shm` sidecars when a
/// connection opens and removes them when the last one closes. Each step
/// bumps the mtime of the store root, so an install that adds nothing to the
/// store would still make CI caches keyed on mtimes (such as GitLab's)
/// re-upload the store.
pub(super) struct RootMtimeGuard {
    root: PathBuf,
    snapshot: Option<RootSnapshot>,
}

struct RootSnapshot {
    mtime: SystemTime,
    entries: Vec<OsString>,
}

/// Share the guard of every connection under `root`, taking the snapshot when
/// the first one opens.
pub(super) fn acquire(root: &Path) -> Arc<RootMtimeGuard> {
    static GUARDS: LazyLock<Mutex<HashMap<PathBuf, Weak<RootMtimeGuard>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let root = dunce::canonicalize(root)
        .or_else(|_| std::path::absolute(root))
        .unwrap_or_else(|_| root.to_path_buf());
    let mut guards = GUARDS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    guards.retain(|_, guard| guard.strong_count() > 0);
    if let Some(guard) = guards.get(&root).and_then(Weak::upgrade) {
        return guard;
    }
    let snapshot = take_snapshot(&root).ok();
    let guard = Arc::new(RootMtimeGuard { root: root.clone(), snapshot });
    guards.insert(root, Arc::downgrade(&guard));
    guard
}

impl Drop for RootMtimeGuard {
    fn drop(&mut self) {
        let Some(snapshot) = &self.snapshot else { return };
        let Ok(current) = take_snapshot(&self.root) else { return };
        if current.entries != snapshot.entries || current.mtime == snapshot.mtime {
            return;
        }
        if let Err(error) = set_dir_mtime(&self.root, snapshot.mtime) {
            tracing::debug!(
                target: "pacquet::store_index",
                ?error,
                root = %self.root.display(),
                "failed to restore the store root's mtime",
            );
        }
    }
}

fn take_snapshot(root: &Path) -> io::Result<RootSnapshot> {
    let mtime = fs::metadata(root)?.modified()?;
    let mut entries = fs::read_dir(root)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_unstable();
    Ok(RootSnapshot { mtime, entries })
}

fn set_dir_mtime(dir: &Path, mtime: SystemTime) -> io::Result<()> {
    let mut options = File::options();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_WRITE_ATTRIBUTES,
        };
        options.access_mode(FILE_WRITE_ATTRIBUTES).custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
    }
    options
        .open(dir)?
        .set_times(FileTimes::new().set_modified(mtime))
}

#[cfg(test)]
mod tests;
