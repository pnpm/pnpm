use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use crate::server::StripedLocks;

use super::IndexFetcher;

/// A cached index file whose fetch lock is held: the stripe of its cache
/// path, released when this is dropped. Only the holder may evict or
/// replace the entry, or it could remove an entry another caller has just
/// refreshed, so both take a [`LockedEntry`] rather than a bare path.
pub(super) struct LockedEntry<'locks> {
    path: PathBuf,
    _guard: tokio::sync::MutexGuard<'locks, ()>,
}

impl<'locks> LockedEntry<'locks> {
    pub(super) async fn lock(locks: &'locks StripedLocks, path: PathBuf) -> Self {
        let guard = locks.lock(&path.to_string_lossy()).await;
        Self { path, _guard: guard }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    /// [`IndexFetcher::cached_entry`], removing a stale entry so resolving
    /// the same crates past their TTL replaces entries instead of
    /// accumulating them.
    pub(super) async fn cached_or_evict(&self, ttl: Duration) -> Option<String> {
        IndexFetcher::cached_entry(&self.path, ttl, true).await
    }
}
