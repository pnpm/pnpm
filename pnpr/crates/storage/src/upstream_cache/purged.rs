use super::{
    CanonicalPackageName, Ecosystem, ErrorKind, Path, PathBuf, Result, Store, fs, write_atomic,
};
use crate::BoundedCache;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{
        Mutex,
        atomic::{AtomicU8, Ordering},
    },
};

/// Subdirectory at the disposable cache root holding durable markers for
/// upstream packages whose authoritative `404` directory removal failed.
pub const PURGED_UPSTREAM_DIR: &str = ".purged";

pub(super) const OBSERVED_MARKER_CACHE_CAPACITY: usize = 4_096;

const DIR_UNKNOWN: u8 = 0;
const DIR_ABSENT: u8 = 1;
const DIR_PRESENT: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkerState {
    Purged,
    Clean,
}

#[derive(Serialize, Deserialize)]
struct PurgedMarker {
    namespace: String,
    ecosystem: Ecosystem,
    package: String,
}

#[derive(Debug, Default)]
struct PurgeCacheState {
    generation: u64,
    active_purged: HashSet<(String, CanonicalPackageName)>,
    observed: BoundedCache<(String, CanonicalPackageName), MarkerState>,
}

#[derive(Debug, Default)]
pub(crate) struct PurgedUpstreamPackages {
    dir_state: AtomicU8,
    state: Mutex<PurgeCacheState>,
}

impl PurgedUpstreamPackages {
    pub(super) async fn is_purged(
        &self,
        store: &Store,
        namespace: &str,
        name: &CanonicalPackageName,
    ) -> Result<bool> {
        if let Some(state) = self.cached_state(namespace, name) {
            return Ok(matches!(state, MarkerState::Purged));
        }
        if self.dir_state.load(Ordering::Acquire) == DIR_ABSENT {
            return Ok(false);
        }
        Box::pin(self.lookup_marker(store, namespace, name)).await
    }

    pub(super) async fn mark_purged(
        &self,
        store: &Store,
        namespace: &str,
        name: &CanonicalPackageName,
    ) -> Result<()> {
        let _write_guard = store.record_write_lock.lock().await;
        let package_dir = store.namespaced(namespace).package_dir(name);
        if let Err(err) = fs::metadata(&package_dir).await
            && err.kind() == ErrorKind::NotFound
        {
            self.set_state(namespace, name, MarkerState::Clean);
            return remove_marker_if_present(&marker_path(&store.root, namespace, name)).await;
        }
        self.dir_state.store(DIR_PRESENT, Ordering::Release);
        self.set_state(namespace, name, MarkerState::Purged);
        let marker = PurgedMarker {
            namespace: namespace.to_string(),
            ecosystem: name.ecosystem(),
            package: name.as_str().to_string(),
        };
        let bytes = serde_json::to_vec(&marker)?;
        write_atomic(&marker_path(&store.root, namespace, name), &bytes).await
    }

    pub(super) async fn clear_purged(
        &self,
        store: &Store,
        namespace: &str,
        name: &CanonicalPackageName,
    ) -> Result<()> {
        let cached = self.cached_state(namespace, name);
        if cached == Some(MarkerState::Clean)
            || (cached.is_none() && self.dir_state.load(Ordering::Acquire) == DIR_ABSENT)
        {
            return Ok(());
        }
        let _write_guard = store.record_write_lock.lock().await;
        remove_marker_if_present(&marker_path(&store.root, namespace, name)).await?;
        self.set_state(namespace, name, MarkerState::Clean);
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn observed_len_for_test(&self) -> usize {
        let state = self.state.lock().expect("purged upstream lock poisoned");
        state.observed.len()
    }

    async fn lookup_marker(
        &self,
        store: &Store,
        namespace: &str,
        name: &CanonicalPackageName,
    ) -> Result<bool> {
        let observed_gen = self.current_generation();
        if self.dir_state.load(Ordering::Acquire) == DIR_UNKNOWN
            && !ensure_dir_present(&store.root, &self.dir_state).await?
        {
            return Ok(self.cached_state(namespace, name) == Some(MarkerState::Purged));
        }
        let disk_state = self.inspect_package_marker(store, namespace, name).await;
        let effective = self.record_observed_state(namespace, name, observed_gen, disk_state);
        Ok(matches!(effective, MarkerState::Purged))
    }

    fn current_generation(&self) -> u64 {
        let state = self.state.lock().expect("purged upstream lock poisoned");
        state.generation
    }

    fn cached_state(&self, namespace: &str, name: &CanonicalPackageName) -> Option<MarkerState> {
        let state = self.state.lock().expect("purged upstream lock poisoned");
        let key = (namespace.to_string(), name.clone());
        if state.active_purged.contains(&key) {
            return Some(MarkerState::Purged);
        }
        state.observed.get(&key).copied()
    }

    fn set_state(&self, namespace: &str, name: &CanonicalPackageName, marker_state: MarkerState) {
        let mut state = self.state.lock().expect("purged upstream lock poisoned");
        let key = (namespace.to_string(), name.clone());
        state.generation += 1;
        match marker_state {
            MarkerState::Purged => {
                state.active_purged.insert(key.clone());
            }
            MarkerState::Clean => {
                state.active_purged.remove(&key);
            }
        }
        state.observed.insert(key, marker_state, 1, OBSERVED_MARKER_CACHE_CAPACITY);
    }

    fn record_observed_state(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        observed_gen: u64,
        disk_state: MarkerState,
    ) -> MarkerState {
        let mut state = self.state.lock().expect("purged upstream lock poisoned");
        let key = (namespace.to_string(), name.clone());
        if state.active_purged.contains(&key) {
            return MarkerState::Purged;
        }
        if state.generation != observed_gen {
            return state.observed
                .get(&key)
                .copied()
                .unwrap_or(disk_state);
        }
        if let Some(existing) = state.observed.get(&key).copied() {
            return existing;
        }
        state.observed.insert(key, disk_state, 1, OBSERVED_MARKER_CACHE_CAPACITY);
        disk_state
    }

    async fn inspect_package_marker(
        &self,
        store: &Store,
        namespace: &str,
        name: &CanonicalPackageName,
    ) -> MarkerState {
        let path = marker_path(&store.root, namespace, name);
        let bytes = match fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == ErrorKind::NotFound => return MarkerState::Clean,
            Err(_) => return MarkerState::Purged,
        };
        let Ok(marker) = serde_json::from_slice::<PurgedMarker>(&bytes) else {
            return MarkerState::Purged;
        };
        if marker.namespace != namespace
            || marker.ecosystem != name.ecosystem()
            || marker.package != name.as_str()
        {
            return MarkerState::Purged;
        }
        let package_dir = store.namespaced(namespace).package_dir(name);
        match fs::metadata(&package_dir).await {
            Err(err) if err.kind() == ErrorKind::NotFound => {
                self.cleanup_orphaned_marker(store, namespace, name, &package_dir, &path).await
            }
            Ok(_) | Err(_) => MarkerState::Purged,
        }
    }

    async fn cleanup_orphaned_marker(
        &self,
        store: &Store,
        namespace: &str,
        name: &CanonicalPackageName,
        package_dir: &Path,
        marker: &Path,
    ) -> MarkerState {
        let _write_guard = store.record_write_lock.lock().await;
        if self.is_active_purged(namespace, name) {
            return MarkerState::Purged;
        }
        match fs::metadata(package_dir).await {
            Err(err) if err.kind() == ErrorKind::NotFound => {
                let _ = remove_marker_if_present(marker).await;
                MarkerState::Clean
            }
            Ok(_) | Err(_) => MarkerState::Purged,
        }
    }

    fn is_active_purged(&self, namespace: &str, name: &CanonicalPackageName) -> bool {
        let state = self.state.lock().expect("purged upstream lock poisoned");
        state.active_purged.contains(&(namespace.to_string(), name.clone()))
    }
}

async fn ensure_dir_present(cache_root: &Path, dir_state: &AtomicU8) -> Result<bool> {
    match fs::metadata(cache_root.join(PURGED_UPSTREAM_DIR)).await {
        Ok(_) => {
            let _ = dir_state.compare_exchange(
                DIR_UNKNOWN,
                DIR_PRESENT,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            Ok(true)
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {
            let _ = dir_state.compare_exchange(
                DIR_UNKNOWN,
                DIR_ABSENT,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            Ok(dir_state.load(Ordering::Acquire) == DIR_PRESENT)
        }
        Err(err) => Err(err.into()),
    }
}

pub(super) fn marker_path(
    cache_root: &Path,
    namespace: &str,
    name: &CanonicalPackageName,
) -> PathBuf {
    let ecosystem = name.ecosystem().as_str();
    let raw = format!(
        "{}:{namespace}:{}:{ecosystem}:{}",
        namespace.len(),
        ecosystem.len(),
        name.as_str(),
    );
    cache_root
        .join(PURGED_UPSTREAM_DIR)
        .join(format!("{}.json", pnpm_crypto_hash::create_hex_hash(&raw)))
}

async fn remove_marker_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}
