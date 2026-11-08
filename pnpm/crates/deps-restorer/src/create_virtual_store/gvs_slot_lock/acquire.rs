use super::{
    ABANDONED_AFTER, GlobalVirtualStoreSlot, GvsSlotLockError, SLOT_LOCK_DIR, WAIT,
    inherited_slot_locks, slot_lock_is_inherited,
};
use pnpm_fs::DirLock;
use pnpm_lockfile::PackageKey;
use std::{
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// How long an install started by a build script waits for a slot lock
/// before it fails. Its parent holds slot locks while the script runs, so an
/// install that holds another slot and waits for one of the parent's could
/// otherwise wait forever.
const NESTED_WAIT: Duration = Duration::from_secs(10);

impl GlobalVirtualStoreSlot {
    pub fn acquire_path(path: PathBuf, key: &PackageKey) -> Result<Self, GvsSlotLockError> {
        let inherited = inherited_slot_locks()
            .map_err(|source| GvsSlotLockError { dep_path: key.to_string(), source })?;
        Self::acquire_path_with_inherited(path, key, &inherited)
    }

    pub(super) fn acquire_path_with_inherited(
        path: PathBuf,
        key: &PackageKey,
        inherited: &[PathBuf],
    ) -> Result<Self, GvsSlotLockError> {
        if slot_lock_is_inherited(&path, inherited)
            .map_err(|source| GvsSlotLockError { dep_path: key.to_string(), source })?
        {
            return Err(nested_lock_error(key));
        }
        let lock_path = path.join(SLOT_LOCK_DIR);
        let lock = if inherited.is_empty() {
            DirLock::acquire_waiting(&lock_path, WAIT, ABANDONED_AFTER)
                .map_err(|source| GvsSlotLockError { dep_path: key.to_string(), source })?
        } else {
            acquire_nested(&lock_path, NESTED_WAIT)
                .map_err(|source| GvsSlotLockError { dep_path: key.to_string(), source })?
        };
        Ok(Self::from_path(path, key, Some(lock)))
    }
}

fn nested_lock_error(key: &PackageKey) -> GvsSlotLockError {
    GvsSlotLockError {
        dep_path: key.to_string(),
        source: io::Error::new(
            io::ErrorKind::WouldBlock,
            "a nested install cannot wait on a slot held by a parent build",
        ),
    }
}

pub(super) fn acquire_nested(lock_path: &Path, wait: Duration) -> io::Result<DirLock> {
    let deadline = Instant::now() + wait;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match DirLock::acquire(lock_path.to_path_buf(), remaining, ABANDONED_AFTER) {
            Ok(Some(lock)) => return Ok(lock),
            Ok(None) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "an install started by a build script waited {}s for a slot held by another install",
                    wait.as_secs(),
                ),
            ));
        }
    }
}
