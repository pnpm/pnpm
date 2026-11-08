use super::{
    ABANDONED_AFTER, GlobalVirtualStoreSlot, GvsSlotLockError, SLOT_LOCK_DIR, WAIT,
    inherited_slot_locks, slot_lock_is_inherited,
};
use pnpm_fs::DirLock;
use pnpm_lockfile::PackageKey;
use std::{io, path::PathBuf, time::Duration};

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
            return Err(nested_lock_error(key, "a parent build"));
        }
        let lock_path = path.join(SLOT_LOCK_DIR);
        let lock = if inherited.is_empty() {
            DirLock::acquire_waiting(&lock_path, WAIT, ABANDONED_AFTER)
                .map_err(|source| GvsSlotLockError { dep_path: key.to_string(), source })?
        } else {
            DirLock::acquire(lock_path, Duration::ZERO, ABANDONED_AFTER)
                .map_err(|source| GvsSlotLockError { dep_path: key.to_string(), source })?
                .ok_or_else(|| nested_lock_error(key, "another install"))?
        };
        Ok(Self::from_path(path, key, Some(lock)))
    }
}

fn nested_lock_error(key: &PackageKey, holder: &str) -> GvsSlotLockError {
    GvsSlotLockError {
        dep_path: key.to_string(),
        source: io::Error::new(
            io::ErrorKind::WouldBlock,
            format!("a nested install cannot wait on a slot held by {holder}"),
        ),
    }
}
