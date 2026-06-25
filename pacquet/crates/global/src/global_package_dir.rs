//! Port of pnpm's
//! [`globalPackageDir`](https://github.com/pnpm/pnpm/blob/1819226b51/global/packages/src/globalPackageDir.ts):
//! the hash-symlink path helpers and the temporary install-directory
//! factory used by global add/update.

use std::{
    io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// The path of the hash symlink for `hash` under `global_dir`.
#[must_use]
pub fn get_hash_link(global_dir: &Path, hash: &str) -> PathBuf {
    global_dir.join(hash)
}

/// Resolve the install directory a hash symlink points at, or `None` when
/// the link is absent or is not a symlink. Mirrors pnpm's
/// `resolveInstallDir`.
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
/// Mirrors pnpm's `createInstallDir`: the directory name is
/// `<pid-hex>-<now-hex>` so concurrent installs don't collide.
pub fn create_install_dir(global_dir: &Path) -> io::Result<PathBuf> {
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let name = format!("{:x}-{:x}", std::process::id(), now_ms);
    let dir = global_dir.join(name);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
