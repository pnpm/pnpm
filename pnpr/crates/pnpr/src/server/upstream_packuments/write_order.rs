use std::sync::{Mutex, atomic::AtomicU64};

use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::BoundedCache;

const WRITE_ORDER_CAPACITY: usize = 16 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::server) enum WriteVerdict {
    /// No newer fetch has committed; this fetch may apply its outcome.
    Proceed,
    /// A newer `200` fetch already persisted a document; return the stored copy.
    SupersededByStored,
    /// A newer authoritative `404` already purged the package; treat as absent
    /// even if local directory removal failed.
    SupersededByPurge,
}

#[derive(Clone, Copy)]
enum CommittedKind {
    Stored,
    Purged,
}

#[derive(Clone, Copy)]
struct CommittedWrite {
    seq: u64,
    kind: CommittedKind,
}

/// Monotonic fetch-start ordering per `(namespace, package)` so an older
/// [`super::PackumentFlightKind::Read`] completion can never overwrite a
/// document stored by a newer [`super::PackumentFlightKind::ForcedRefresh`] or
/// resurrect an authoritative upstream `404` purge.
pub(in crate::server) struct PackumentWriteOrder {
    next_seq: AtomicU64,
    accepted: Mutex<BoundedCache<(String, CanonicalPackageName), CommittedWrite>>,
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

    /// Compare `fetch_seq` against the last committed outcome for
    /// `(namespace, name)`. Call under [`super::lock_upstream_package`].
    pub(in crate::server) fn check(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        fetch_seq: u64,
    ) -> WriteVerdict {
        let accepted = self.accepted.lock().expect("packument write-order lock poisoned");
        match accepted
            .get(&(namespace.to_string(), name.clone()))
            .copied()
        {
            Some(committed) if fetch_seq <= committed.seq => match committed.kind {
                CommittedKind::Stored => WriteVerdict::SupersededByStored,
                CommittedKind::Purged => WriteVerdict::SupersededByPurge,
            },
            _ => WriteVerdict::Proceed,
        }
    }

    /// Whether the latest committed outcome for `(namespace, name)` is an
    /// authoritative `404` purge.
    pub(in crate::server) fn is_purged(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
    ) -> bool {
        let accepted = self.accepted.lock().expect("packument write-order lock poisoned");
        matches!(
            accepted.get(&(namespace.to_string(), name.clone())),
            Some(CommittedWrite { kind: CommittedKind::Purged, .. }),
        )
    }

    /// Commit `fetch_seq` after a `200` document persistence succeeds under
    /// [`super::lock_upstream_package`].
    pub(in crate::server) fn commit_stored(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        fetch_seq: u64,
    ) {
        self.commit_kind(namespace, name, fetch_seq, CommittedKind::Stored);
    }

    /// Commit `fetch_seq` for an authoritative upstream `404` under
    /// [`super::lock_upstream_package`], even if local cache removal failed.
    pub(in crate::server) fn commit_purged(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        fetch_seq: u64,
    ) {
        self.commit_kind(namespace, name, fetch_seq, CommittedKind::Purged);
    }

    fn commit_kind(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        fetch_seq: u64,
        kind: CommittedKind,
    ) {
        let mut accepted = self.accepted.lock().expect("packument write-order lock poisoned");
        let key = (namespace.to_string(), name.clone());
        let last_seq = accepted.get(&key).map_or(0, |entry| entry.seq);
        if fetch_seq > last_seq {
            accepted.insert(key, CommittedWrite { seq: fetch_seq, kind }, 1, WRITE_ORDER_CAPACITY);
        }
    }
}
