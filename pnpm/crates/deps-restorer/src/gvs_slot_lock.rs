//! The lock that serializes writes into one slot of the global virtual
//! store across processes.
//!
//! Every project whose dependency graph hashes to a slot shares its
//! directory, so concurrent installs would otherwise run the same
//! lifecycle script in it at once, or re-import pristine files over a
//! build another install is still running. Both writers take this lock
//! while the slot carries [`crate::NEEDS_BUILD_MARKER`].

use crate::VirtualStoreLayout;
use pnpm_fs::DirLock;
use pnpm_lockfile::PackageKey;
use std::time::Duration;

/// The lock directory's name inside the slot directory, next to its
/// `node_modules`.
const SLOT_LOCK_DIR: &str = ".pnpm-build.lock";

/// How long a writer waits for another process's build of the slot
/// before it writes unserialized.
const WAIT: Duration = Duration::from_mins(10);

/// Comfortably above how long a build can legitimately take, so a holder
/// that cannot prove it is alive never has its lock stolen mid-build.
const ABANDONED_AFTER: Duration = Duration::from_mins(30);

/// Take the lock of `key`'s slot. `None` when the global virtual store
/// is off, or the lock could not be taken, in which case the caller
/// proceeds unserialized.
pub(crate) fn lock_global_virtual_store_slot(
    layout: &VirtualStoreLayout,
    key: &PackageKey,
) -> Option<DirLock> {
    if !layout.enable_global_virtual_store() {
        return None;
    }
    let path = layout.slot_dir(key).join(SLOT_LOCK_DIR);
    match DirLock::acquire(path, WAIT, ABANDONED_AFTER) {
        Ok(Some(lock)) => Some(lock),
        Ok(None) => {
            tracing::debug!(
                target: "pacquet::build",
                dep_path = %key,
                "timed out waiting for the global virtual store slot lock",
            );
            None
        }
        Err(error) => {
            tracing::debug!(
                target: "pacquet::build",
                ?error,
                dep_path = %key,
                "failed to take the global virtual store slot lock",
            );
            None
        }
    }
}
