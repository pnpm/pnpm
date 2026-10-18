//! The per-target lock that serializes the swaps of one occupied target
//! across installs.

use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};

/// How long a swap waits for another process's swap of the same target
/// before it replaces the target unserialized, as the global virtual
/// store slot lock does. A swap takes seconds at most, so a wait this
/// long only runs out on a holder that is stuck.
const TARGET_SWAP_LOCK_WAIT: Duration = Duration::from_mins(10);
/// Past how long a swap can legitimately take, so a holder that cannot
/// prove it is alive never has its lock stolen mid-swap.
const TARGET_SWAP_LOCK_ABANDONED_AFTER: Duration = Duration::from_mins(30);

/// Serialize the replacement of `dir_path` with the other installs
/// replacing it. `None` when the lock cannot be taken in time or at all,
/// in which case the caller replaces the target unserialized: the swap
/// tolerates a concurrent importer, the lock only keeps the two from
/// undoing each other's work, so a lock directory this user cannot use
/// must not fail the install.
pub(super) fn target_swap_lock(dir_path: &Path) -> Option<pnpm_fs::DirLock> {
    let acquired = target_swap_lock_path(dir_path)
        .and_then(|lock_path| {
            pnpm_fs::DirLock::acquire(
                lock_path,
                TARGET_SWAP_LOCK_WAIT,
                TARGET_SWAP_LOCK_ABANDONED_AFTER,
            )
        });
    match acquired {
        Ok(Some(lock)) => Some(lock),
        Ok(None) => {
            tracing::debug!(
                target: "pacquet::import_indexed_dir",
                ?dir_path,
                "timed out waiting for the target swap lock; replacing the target unserialized",
            );
            None
        }
        Err(error) => {
            tracing::debug!(
                target: "pacquet::import_indexed_dir",
                ?dir_path,
                %error,
                "could not take the target swap lock; replacing the target unserialized",
            );
            None
        }
    }
}

pub(super) fn target_swap_lock_path(dir_path: &Path) -> Result<PathBuf, io::Error> {
    let normalized_path = pnpm_fs::lexical_normalize(dir_path);
    let target_name = normalized_path
        .file_name()
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "target path has no file name")
        })?;
    let parent = normalized_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let resolved_path = pnpm_fs::realpath_missing(parent)?.join(target_name);
    pnpm_fs::secure_user_lock_file_path("pnpm-indexed-package-swaps", &resolved_path, "lock")
}
