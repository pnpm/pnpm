use super::{
    ABANDONED_AFTER, GlobalVirtualStoreSlot, GvsSlotLockError, SLOT_LOCK_DIR, SlotAttempt, WAIT,
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

    pub(super) fn try_acquire_path(
        path: PathBuf,
        key: &PackageKey,
    ) -> Result<SlotAttempt, GvsSlotLockError> {
        let inherited = inherited_slot_locks()
            .map_err(|source| GvsSlotLockError { dep_path: key.to_string(), source })?;
        if slot_lock_is_inherited(&path, &inherited)
            .map_err(|source| GvsSlotLockError { dep_path: key.to_string(), source })?
        {
            return Err(nested_lock_error(key));
        }
        match DirLock::acquire(path.join(SLOT_LOCK_DIR), Duration::ZERO, ABANDONED_AFTER) {
            Ok(Some(lock)) => {
                Ok(SlotAttempt::Acquired(Some(Self::from_path(path, key, Some(lock)))))
            }
            Ok(None) => Ok(SlotAttempt::Busy),
            // A prune removed the slot between the attempt's steps.
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(SlotAttempt::Busy),
            Err(source) => Err(GvsSlotLockError { dep_path: key.to_string(), source }),
        }
    }
}

/// When a caller retrying [`GlobalVirtualStoreSlot::try_acquire`] for `key`
/// gives up: `None` for a top-level install, which keeps retrying, and the
/// end of [`NESTED_WAIT`] for an install started by a build script.
pub(crate) fn slot_retry_deadline(key: &PackageKey) -> Result<Option<Instant>, GvsSlotLockError> {
    let inherited = inherited_slot_locks()
        .map_err(|source| GvsSlotLockError { dep_path: key.to_string(), source })?;
    Ok((!inherited.is_empty()).then(|| Instant::now() + NESTED_WAIT))
}

/// The error for `key` once its [`slot_retry_deadline`] has passed.
pub(crate) fn slot_retry_expired(key: &PackageKey) -> GvsSlotLockError {
    GvsSlotLockError { dep_path: key.to_string(), source: nested_wait_expired(NESTED_WAIT) }
}

fn nested_wait_expired(wait: Duration) -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "an install started by a build script waited {}s for a slot held by another install",
            wait.as_secs(),
        ),
    )
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
            return Err(nested_wait_expired(wait));
        }
    }
}
