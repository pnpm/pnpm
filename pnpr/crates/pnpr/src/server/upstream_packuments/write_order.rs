use std::sync::{Mutex, atomic::AtomicU64};

use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::BoundedCache;

const WRITE_ORDER_CAPACITY: usize = 16 * 1024;

/// Monotonic fetch-start ordering per `(namespace, package)` so an older
/// [`super::PackumentFlightKind::Read`] completion can never overwrite a
/// document stored by a newer [`super::PackumentFlightKind::ForcedRefresh`]
/// (while still allowing a refresh started while a read was in flight to
/// replace that read's write).
pub(in crate::server) struct PackumentWriteOrder {
    next_seq: AtomicU64,
    accepted: Mutex<BoundedCache<(String, CanonicalPackageName), u64>>,
}

impl Default for PackumentWriteOrder {
    fn default() -> Self {
        Self { next_seq: AtomicU64::new(1), accepted: Mutex::default() }
    }
}

impl PackumentWriteOrder {
    pub(in crate::server) fn begin_fetch(&self) -> u64 {
        self.next_seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Whether `fetch_seq` is newer than the last committed persistence for
    /// `(namespace, name)`. Call under [`super::lock_upstream_package`].
    pub(in crate::server) fn accepts(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        fetch_seq: u64,
    ) -> bool {
        let accepted = self.accepted.lock().expect("packument write-order lock poisoned");
        let last_seq = accepted
            .get(&(namespace.to_string(), name.clone()))
            .copied()
            .unwrap_or(0);
        fetch_seq > last_seq
    }

    /// Commit `fetch_seq` after storage persistence succeeds under
    /// [`super::lock_upstream_package`], so a failed parse or write never
    /// suppresses a pending valid fetch.
    pub(in crate::server) fn commit(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        fetch_seq: u64,
    ) {
        let mut accepted = self.accepted.lock().expect("packument write-order lock poisoned");
        let key = (namespace.to_string(), name.clone());
        let last_seq = accepted.get(&key).copied().unwrap_or(0);
        if fetch_seq > last_seq {
            accepted.insert(key, fetch_seq, 1, WRITE_ORDER_CAPACITY);
        }
    }
}
