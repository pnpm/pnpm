//! The offline pick's store view: which versions of a packument the store
//! already holds, decided once per packument route and shared by every later
//! pick that asks the same question.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
};

/// The memo per packument route and scan range: the packument snapshot the
/// narrowing was derived from, plus the narrowed packument itself.
type NarrowedMemo = HashMap<(String, String), (Arc<Package>, Option<Arc<Package>>)>;

use pnpm_registry::PackageDistribution;
use pnpm_store_dir::{SharedReadonlyStoreIndex, StoreDir, StoreIndex, store_index_key};

use super::Package;
use crate::{
    npm_resolver::dist_integrity,
    pick_package_from_meta::{filter_pkg_metadata_versions, semver_range::semver_satisfies_loose},
};

/// The install's store index plus a memo of what it holds. Offline installs
/// never fetch, so the store cannot gain rows while the resolve runs: once a
/// key's presence is decided, the answer holds for the whole install.
pub struct OfflineStoreView {
    index: SharedReadonlyStoreIndex,
    /// Store-index key → whether the index holds it.
    presence: Mutex<HashMap<String, bool>>,
    /// (Packument route, scan range) → the packument narrowed to its in-store
    /// versions within that range.
    narrowed: Mutex<NarrowedMemo>,
}

impl OfflineStoreView {
    #[must_use]
    pub fn new(index: SharedReadonlyStoreIndex) -> Self {
        Self { index, presence: Mutex::new(HashMap::new()), narrowed: Mutex::new(HashMap::new()) }
    }

    /// The view an offline resolve consults: `None` when the resolve is
    /// online, or when the store has no index yet and so holds nothing to
    /// prefer. `frozen_store` selects the read-only open mode
    /// [`StoreIndex::shared_for`] documents.
    #[must_use]
    pub fn open_for_offline(
        offline: bool,
        store_dir: &StoreDir,
        frozen_store: bool,
    ) -> Option<Self> {
        if !offline {
            return None;
        }
        StoreIndex::shared_for(store_dir, frozen_store).map(Self::new)
    }

    /// The raw store index handle for the lockfile-pinned peek fast path.
    pub(crate) fn index(&self) -> &SharedReadonlyStoreIndex {
        &self.index
    }

    /// Whether the store index holds the row for a picked version's
    /// `integrity\tname@version` key. The query runs on the blocking pool,
    /// because the index mutex may be held by another pick's batched
    /// [`Self::narrowed`] query. The answer is memoized, so repeat picks of
    /// the same version are a hash lookup.
    pub(crate) async fn holds(&self, key: &str) -> bool {
        if let Ok(presence) = self.presence.lock()
            && let Some(cached) = presence.get(key).copied()
        {
            return cached;
        }
        let index = Arc::clone(&self.index);
        let owned_key = key.to_string();
        let present = tokio::task::spawn_blocking(move || {
            index
                .lock()
                .ok()
                .and_then(|guard| guard.contains_key(&owned_key).ok())
        })
        .await
        .ok()
        .flatten()
        .unwrap_or(false);
        if let Ok(mut presence) = self.presence.lock() {
            presence.insert(key.to_string(), present);
        }
        present
    }

    /// The packument narrowed to the versions within `scan_range` whose
    /// tarball the store already holds, or `None` when no version qualifies
    /// so the caller keeps the unrestricted pick. `route_key` identifies the
    /// packument (registry, name, metadata kind), so picks of one document
    /// and range share a single decision, computed in one trip to the index.
    /// A `scan_range` of `*` considers every version.
    pub(super) async fn narrowed(
        &self,
        route_key: &str,
        scan_range: &str,
        meta: &Arc<Package>,
    ) -> Option<Arc<Package>> {
        let memo_key = (route_key.to_string(), scan_range.to_string());
        if let Some(cached) = self
            .locked()
            .and_then(|guard| guard.get(&memo_key).cloned())
        {
            // `update_checksums` bypasses the metadata cache, so the same
            // route can present a different packument snapshot: reuse the
            // memo only when it was derived from this exact document.
            if Arc::ptr_eq(&cached.0, meta) {
                return cached.1;
            }
        }
        let computed = self.compute(meta, scan_range).await;
        if let Some(mut guard) = self.locked() {
            guard.insert(memo_key, (Arc::clone(meta), computed.clone()));
        }
        computed
    }

    fn locked(&self) -> Option<MutexGuard<'_, NarrowedMemo>> {
        self.narrowed.lock().ok()
    }

    /// One trip to the store index for every version of `meta` within
    /// `scan_range` that has a [`tarball_key`]. Building a key hydrates the
    /// version's manifest, so the range is checked on the version string
    /// first, the same way the re-pick applies it.
    async fn compute(&self, meta: &Package, scan_range: &str) -> Option<Arc<Package>> {
        let keys_by_version: HashMap<String, String> = meta.versions
            .keys()
            .filter(|version| scan_range == "*" || semver_satisfies_loose(version, scan_range))
            .filter_map(|version| {
                let pkg_version = meta.versions.get(version)?;
                Some((version.clone(), tarball_key(&meta.name, version, &pkg_version.dist)?))
            })
            .collect();
        if keys_by_version.is_empty() {
            return None;
        }
        let keys: Vec<String> = keys_by_version
            .values()
            .cloned()
            .collect();
        let index = Arc::clone(&self.index);
        let held: HashSet<String> = tokio::task::spawn_blocking(move || {
            let guard = index.lock().ok()?;
            guard.contains_many(&keys).ok()
        })
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
        if held.is_empty() {
            return None;
        }
        Some(Arc::new(filter_pkg_metadata_versions(meta, |version| {
            keys_by_version
                .get(version)
                .is_some_and(|key| held.contains(key))
        })))
    }
}

/// The store-index key a version's tarball is written under, from the same
/// integrity the resolution pins: `dist.integrity`, or the `sha1-` form of a
/// legacy `dist.shasum`. `None` when the version pins nothing usable, so the
/// store can never hold it.
pub(super) fn tarball_key(name: &str, version: &str, dist: &PackageDistribution) -> Option<String> {
    let integrity = dist_integrity(dist).ok().flatten()?;
    Some(store_index_key(&integrity.to_string(), &format!("{name}@{version}")))
}
