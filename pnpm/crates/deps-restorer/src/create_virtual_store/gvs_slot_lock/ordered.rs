use super::{
    ABANDONED_AFTER, GlobalVirtualStoreSlot, GvsSlotLockError, SLOT_LOCK_DIR, inherited_slot_locks,
    path_codec::slot_lock_is_inherited,
};
use crate::VirtualStoreLayout;
use pnpm_fs::DirLock;
use pnpm_lockfile::PackageKey;
use rayon::prelude::*;
use std::{fs, io, path::PathBuf, time::Duration};

enum ExistingSlotLockAttempt {
    Acquired(Option<Box<GlobalVirtualStoreSlot>>),
    Busy,
}

impl GlobalVirtualStoreSlot {
    pub fn acquire_ordered(
        layout: &VirtualStoreLayout,
        keys: impl IntoIterator<Item = PackageKey>,
    ) -> Result<Vec<Self>, GvsSlotLockError> {
        let mut keys = keys.into_iter().collect::<Vec<_>>();
        keys.sort_by_key(|key| layout.slot_dir(key));
        keys.dedup_by(|left, right| layout.slot_dir(left) == layout.slot_dir(right));
        let mut slots = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some(slot) = Self::acquire(layout, &key)? {
                slots.push(slot);
            }
        }
        Ok(slots)
    }

    pub fn acquire_existing_ordered(
        layout: &VirtualStoreLayout,
        keys: impl IntoIterator<Item = PackageKey>,
    ) -> Result<Vec<Self>, GvsSlotLockError> {
        let mut keys = keys.into_iter().collect::<Vec<_>>();
        keys.sort_by_key(|key| layout.slot_dir(key));
        keys.dedup_by(|left, right| layout.slot_dir(left) == layout.slot_dir(right));
        let mut slots = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some(slot) = Self::acquire_existing(layout, &key)? {
                slots.push(slot);
            }
        }
        Ok(slots)
    }

    pub(crate) fn try_acquire_existing_ordered(
        layout: &VirtualStoreLayout,
        keys: impl IntoIterator<Item = PackageKey>,
    ) -> Result<Option<Vec<Self>>, GvsSlotLockError> {
        let mut keys = keys.into_iter().collect::<Vec<_>>();
        keys.sort_by_key(|key| layout.slot_dir(key));
        keys.dedup_by(|left, right| layout.slot_dir(left) == layout.slot_dir(right));
        let attempts = keys
            .par_iter()
            .map(|key| Self::try_acquire_existing(layout, key))
            .collect::<Vec<_>>();
        let mut slots = Vec::with_capacity(attempts.len());
        let mut busy = false;
        for attempt in attempts {
            match attempt? {
                ExistingSlotLockAttempt::Acquired(Some(slot)) => slots.push(*slot),
                ExistingSlotLockAttempt::Acquired(None) => {}
                ExistingSlotLockAttempt::Busy => busy = true,
            }
        }
        if busy {
            return Ok(None);
        }
        Ok(Some(slots))
    }

    fn try_acquire_existing(
        layout: &VirtualStoreLayout,
        key: &PackageKey,
    ) -> Result<ExistingSlotLockAttempt, GvsSlotLockError> {
        let Some(path) = layout.hashed_slot_dir(key) else {
            return Ok(ExistingSlotLockAttempt::Acquired(None));
        };
        if layout.frozen_store() {
            return Self::open_existing_path(path, key)
                .map(|slot| ExistingSlotLockAttempt::Acquired(slot.map(Box::new)));
        }
        if !path.is_dir() {
            return Ok(ExistingSlotLockAttempt::Acquired(None));
        }
        let Some(slot) = Self::try_acquire_path(path, key)? else {
            return Ok(ExistingSlotLockAttempt::Busy);
        };
        if slot
            .is_pruned_shell()
            .map_err(|source| GvsSlotLockError { dep_path: key.to_string(), source })?
        {
            let path = slot.path.clone();
            drop(slot);
            match fs::remove_dir(path) {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty,
                    ) => {}
                Err(source) => {
                    return Err(GvsSlotLockError { dep_path: key.to_string(), source });
                }
            }
            return Ok(ExistingSlotLockAttempt::Acquired(None));
        }
        Ok(ExistingSlotLockAttempt::Acquired(Some(Box::new(slot))))
    }

    fn try_acquire_path(path: PathBuf, key: &PackageKey) -> Result<Option<Self>, GvsSlotLockError> {
        let inherited = inherited_slot_locks()
            .map_err(|source| GvsSlotLockError { dep_path: key.to_string(), source })?;
        if slot_lock_is_inherited(&path, &inherited)
            .map_err(|source| GvsSlotLockError { dep_path: key.to_string(), source })?
        {
            return Err(GvsSlotLockError {
                dep_path: key.to_string(),
                source: io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "a nested install cannot wait on a slot held by its parent build",
                ),
            });
        }
        let lock = match DirLock::acquire(path.join(SLOT_LOCK_DIR), Duration::ZERO, ABANDONED_AFTER)
        {
            Ok(lock) => lock,
            Err(error) if error.kind() == io::ErrorKind::NotFound && inherited.is_empty() => None,
            Err(source) => {
                return Err(GvsSlotLockError { dep_path: key.to_string(), source });
            }
        };
        if lock.is_none() && !inherited.is_empty() {
            return Err(GvsSlotLockError {
                dep_path: key.to_string(),
                source: io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "a nested install cannot wait on a slot held by another install",
                ),
            });
        }
        Ok(lock.map(|lock| Self::from_path(path, key, Some(lock))))
    }
}
