use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::AtomicU64,
    },
};

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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::server) enum PurgeRemoval {
    Complete,
    Pending,
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

#[derive(Default)]
struct OrderState {
    accepted: BoundedCache<(String, CanonicalPackageName), CommittedWrite>,
    unresolved_purges: HashMap<(String, CanonicalPackageName), u64>,
}

/// Monotonic fetch-start ordering per `(namespace, package)` so an older
/// [`super::PackumentFlightKind::Read`] completion can never overwrite a
/// document stored by a newer [`super::PackumentFlightKind::ForcedRefresh`] or
/// resurrect an authoritative upstream `404` purge.
pub(in crate::server) struct PackumentWriteOrder {
    next_seq: AtomicU64,
    state: Mutex<OrderState>,
}

impl Default for PackumentWriteOrder {
    fn default() -> Self {
        Self { next_seq: AtomicU64::new(1), state: Mutex::default() }
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
        namespace: String,
        name: &CanonicalPackageName,
        fetch_seq: u64,
    ) -> WriteVerdict {
        let state = self.state.lock().expect("packument write-order lock poisoned");
        let key = (namespace, name.clone());
        if let Some(&purged_seq) = state.unresolved_purges.get(&key)
            && fetch_seq <= purged_seq
        {
            return WriteVerdict::SupersededByPurge;
        }
        match state.accepted.get(&key).copied() {
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
        namespace: String,
        name: &CanonicalPackageName,
    ) -> bool {
        let state = self.state.lock().expect("packument write-order lock poisoned");
        let key = (namespace, name.clone());
        state.unresolved_purges.contains_key(&key)
            || matches!(
                state.accepted.get(&key),
                Some(CommittedWrite { kind: CommittedKind::Purged, .. }),
            )
    }

    /// Commit `fetch_seq` after a `200` document persistence succeeds under
    /// [`super::lock_upstream_package`].
    pub(in crate::server) fn commit_stored(
        &self,
        namespace: String,
        name: &CanonicalPackageName,
        fetch_seq: u64,
    ) {
        let mut state = self.state.lock().expect("packument write-order lock poisoned");
        let key = (namespace, name.clone());
        if fetch_seq > latest_seq(&state, &key) {
            state.unresolved_purges.remove(&key);
            state.accepted.insert(
                key,
                CommittedWrite { seq: fetch_seq, kind: CommittedKind::Stored },
                1,
                WRITE_ORDER_CAPACITY,
            );
        }
    }

    /// Commit `fetch_seq` for an authoritative upstream `404` under
    /// [`super::lock_upstream_package`]. Unresolved removals stay tracked
    /// outside the bounded LRU until removal or a newer store succeeds.
    pub(in crate::server) fn commit_purged(
        &self,
        namespace: String,
        name: &CanonicalPackageName,
        fetch_seq: u64,
        removal: PurgeRemoval,
    ) {
        let mut state = self.state.lock().expect("packument write-order lock poisoned");
        let key = (namespace, name.clone());
        if fetch_seq > latest_seq(&state, &key) {
            match removal {
                PurgeRemoval::Complete => {
                    state.unresolved_purges.remove(&key);
                }
                PurgeRemoval::Pending => {
                    state.unresolved_purges.insert(key.clone(), fetch_seq);
                }
            }
            state.accepted.insert(
                key,
                CommittedWrite { seq: fetch_seq, kind: CommittedKind::Purged },
                1,
                WRITE_ORDER_CAPACITY,
            );
        } else {
            match removal {
                PurgeRemoval::Complete => {
                    if state.unresolved_purges
                        .get(&key)
                        .is_some_and(|&seq| seq <= fetch_seq)
                    {
                        state.unresolved_purges.remove(&key);
                    }
                }
                PurgeRemoval::Pending => {}
            }
        }
    }

    /// Mark a previously failed purge's pending directory removal as resolved
    /// under [`super::lock_upstream_package`].
    pub(in crate::server) fn resolve_purge(&self, namespace: String, name: &CanonicalPackageName) {
        let mut state = self.state.lock().expect("packument write-order lock poisoned");
        state.unresolved_purges.remove(&(namespace, name.clone()));
    }
}

fn latest_seq(state: &OrderState, key: &(String, CanonicalPackageName)) -> u64 {
    let accepted = state.accepted
        .get(key)
        .map(|entry| entry.seq)
        .unwrap_or(0);
    let unresolved = state.unresolved_purges
        .get(key)
        .copied()
        .unwrap_or(0);
    accepted.max(unresolved)
}
