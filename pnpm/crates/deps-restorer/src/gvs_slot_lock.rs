//! The lock that serializes writes into one slot of the global virtual
//! store across processes.
//!
//! Every project whose dependency graph hashes to a slot shares its
//! directory, so concurrent installs would otherwise run the same
//! lifecycle script in it at once, or re-import pristine files over a
//! build another install is still running. Both writers take this lock
//! while the slot carries [`crate::NEEDS_BUILD_MARKER`].

use crate::VirtualStoreLayout;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_fs::DirLock;
use pnpm_lockfile::PackageKey;
use std::{io, time::Duration};

/// The lock directory's name inside the slot directory, next to its
/// `node_modules`.
const SLOT_LOCK_DIR: &str = ".pnpm-build.lock";

/// How long each acquisition attempt waits before checking the lock again.
const WAIT: Duration = Duration::from_mins(10);

/// Comfortably above how long a build can legitimately take, so a holder
/// that cannot prove it is alive never has its lock stolen mid-build.
const ABANDONED_AFTER: Duration = Duration::from_mins(30);

#[derive(Debug, Display, Error, Diagnostic)]
#[display("Failed to lock global virtual store slot for {dep_path}: {source}")]
#[diagnostic(code(ERR_PNPM_GVS_SLOT_LOCK))]
pub(crate) struct GvsSlotLockError {
    dep_path: String,
    #[error(source)]
    source: io::Error,
}

/// Take the lock of `key`'s slot. `None` only when the global virtual
/// store is off. A timeout starts another wait; an acquisition error
/// prevents the caller from mutating the shared slot.
pub(crate) fn lock_global_virtual_store_slot(
    layout: &VirtualStoreLayout,
    key: &PackageKey,
) -> Result<Option<DirLock>, GvsSlotLockError> {
    if !layout.enable_global_virtual_store() {
        return Ok(None);
    }
    let path = layout.slot_dir(key).join(SLOT_LOCK_DIR);
    acquire_until_locked(|| DirLock::acquire(path.clone(), WAIT, ABANDONED_AFTER))
        .map(Some)
        .map_err(|source| GvsSlotLockError { dep_path: key.to_string(), source })
}

fn acquire_until_locked<T>(mut acquire: impl FnMut() -> io::Result<Option<T>>) -> io::Result<T> {
    loop {
        match acquire()? {
            Some(lock) => return Ok(lock),
            None => tracing::debug!(
                target: "pacquet::build",
                "still waiting for the global virtual store slot lock",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::acquire_until_locked;
    use std::{cell::Cell, io};

    #[test]
    fn a_timed_out_slot_lock_is_retried() {
        let attempts = Cell::new(0);
        let lock = acquire_until_locked(|| {
            attempts.set(attempts.get() + 1);
            Ok((attempts.get() == 2).then_some("lock"))
        })
        .expect("second attempt acquires the lock");

        assert_eq!(lock, "lock");
        assert_eq!(attempts.get(), 2);
    }

    #[test]
    fn a_slot_lock_error_is_returned() {
        let error = acquire_until_locked::<()>(|| Err(io::Error::other("denied")))
            .expect_err("lock errors must stop slot mutation");

        assert_eq!(error.kind(), io::ErrorKind::Other);
    }
}
