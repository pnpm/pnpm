use super::{GlobalVirtualStoreSlot, GvsSlotLockError};
use crate::VirtualStoreLayout;
use pnpm_lockfile::PackageKey;

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
}
