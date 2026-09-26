//! The offline pick's store view: which versions of a packument the store
//! already holds, decided once per packument route and shared by every later
//! pick that asks the same question.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use pnpm_store_dir::{SharedReadonlyStoreIndex, store_index_key};

use super::Package;
use crate::pick_package_from_meta::filter_pkg_metadata_versions;

/// The install's store index plus a memo of what it holds. Offline installs
/// never fetch, so the store cannot gain rows while the resolve runs: once a
/// key's presence is decided, the answer holds for the whole install.
pub struct OfflineStoreView {
    index: SharedReadonlyStoreIndex,
    /// Store-index key → whether the index holds it.
    presence: Mutex<HashMap<String, bool>>,
    /// Packument route → the packument narrowed to its in-store versions.
    narrowed: Mutex<HashMap<String, Option<Arc<Package>>>>,
}

impl OfflineStoreView {
    #[must_use]
    pub fn new(index: SharedReadonlyStoreIndex) -> Self {
        Self { index, presence: Mutex::new(HashMap::new()), narrowed: Mutex::new(HashMap::new()) }
    }

    /// The raw store index handle for the lockfile-pinned peek fast path.
    pub(crate) fn index(&self) -> &SharedReadonlyStoreIndex {
        &self.index
    }

    /// Whether the store index holds the row for a picked version's
    /// `integrity\tname@version` key. A point query costs a few dozen
    /// microseconds, so it runs inline instead of paying a blocking-pool
    /// hop, and the answer is memoized: repeat picks of the same version
    /// are a hash lookup.
    #[must_use]
    pub(crate) fn holds(&self, key: &str) -> bool {
        if let Ok(presence) = self.presence.lock()
            && let Some(cached) = presence.get(key).copied()
        {
            return cached;
        }
        let present = self.index
            .lock()
            .ok()
            .and_then(|guard| guard.contains_key(key).ok())
            .unwrap_or(false);
        if let Ok(mut presence) = self.presence.lock() {
            presence.insert(key.to_string(), present);
        }
        present
    }

    /// The packument narrowed to the versions whose tarball the store already
    /// holds, or `None` when no version qualifies so the caller keeps the
    /// unrestricted pick. `route_key` identifies the packument (registry,
    /// name, metadata kind), so picks of one document share a single
    /// decision, computed in one trip to the index.
    pub(super) async fn narrowed(
        &self,
        route_key: &str,
        meta: &Arc<Package>,
    ) -> Option<Arc<Package>> {
        if let Some(cached) = self
            .locked()
            .and_then(|guard| guard.get(route_key).cloned())
        {
            return cached;
        }
        let computed = self.compute(meta).await;
        if let Some(mut guard) = self.locked() {
            guard.insert(route_key.to_string(), computed.clone());
        }
        computed
    }

    fn locked(&self) -> Option<std::sync::MutexGuard<'_, HashMap<String, Option<Arc<Package>>>>> {
        self.narrowed.lock().ok()
    }

    /// One trip to the store index for every version of `meta` that carries an
    /// integrity. The store decides membership; the pick applies the range and
    /// every other preference over what remains, so no version parsing happens
    /// here.
    async fn compute(&self, meta: &Package) -> Option<Arc<Package>> {
        let keys_by_version: HashMap<String, String> = meta.versions
            .iter()
            .filter_map(|(version, pkg_version)| {
                let integrity = pkg_version.dist.integrity.as_ref()?;
                Some((
                    version.clone(),
                    store_index_key(&integrity.to_string(), &format!("{}@{}", meta.name, version)),
                ))
            })
            .collect();
        let keys: Vec<String> = keys_by_version
            .values()
            .cloned()
            .collect();
        let index = Arc::clone(&self.index);
        let held: HashSet<String> = tokio::task::spawn_blocking(move || {
            let guard = index.lock().ok()?;
            guard
                .get_many(&keys)
                .ok()
                .map(|hits| hits.into_keys().collect())
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
