//! The hash-symlink path helpers and the temporary install-directory
//! factory used by global add/update.

use std::{
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// The path of the hash symlink for `hash` under `global_dir`.
#[must_use]
pub fn get_hash_link(global_dir: &Path, hash: &str) -> PathBuf {
    global_dir.join(hash)
}

/// Resolve the install directory a hash symlink points at, or `None` when
/// the link is absent or is not a symlink.
pub fn resolve_install_dir(global_dir: &Path, hash: &str) -> io::Result<Option<PathBuf>> {
    let link_path = get_hash_link(global_dir, hash);
    let metadata = match std::fs::symlink_metadata(&link_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_symlink() {
        return Ok(None);
    }
    std::fs::canonicalize(&link_path).map(Some)
}

/// Create and return a fresh, unique install directory under `global_dir`.
///
/// The directory is a per-group dir named from the pid plus high-resolution
/// nanos and an atomic counter, and is created with exclusive `create_dir`
/// (retrying on `AlreadyExists`) rather than `create_dir_all`. This avoids
/// reusing — or following a pre-existing symlink at — a colliding name when
/// `global_dir` is shared, so each group's `package.json` / `node_modules`
/// stay isolated. The parent `global_dir` must already exist.
pub fn create_install_dir(global_dir: &Path) -> io::Result<PathBuf> {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let pid = std::process::id();
    let mut last_err = None;
    for _ in 0..10 {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = global_dir.join(format!("{pid:x}-{nanos:x}-{seq:x}"));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                last_err = Some(error);
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_err
        .unwrap_or_else(|| io::Error::other("could not create a unique global install dir")))
}
