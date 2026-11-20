use super::{
    CanonicalPackageName, Ecosystem, ErrorKind, Path, PathBuf, Result, Store, fs, write_atomic,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
};

/// Subdirectory at the disposable cache root holding durable markers for
/// upstream packages whose authoritative `404` directory removal failed.
pub const PURGED_UPSTREAM_DIR: &str = ".purged";

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
pub(crate) struct PurgedUpstreamPackages {
    dir_state: AtomicU8,
    generation: AtomicU64,
    entries: Mutex<HashMap<(String, CanonicalPackageName), MarkerState>>,
}

impl PurgedUpstreamPackages {
    pub(super) async fn is_purged(
        &self,
        cache_root: &Path,
        namespace: &str,
        name: &CanonicalPackageName,
    ) -> Result<bool> {
        if let Some(state) = self.cached_state(namespace, name) {
            return Ok(matches!(state, MarkerState::Purged));
        }
        if self.dir_state.load(Ordering::Acquire) == DIR_ABSENT {
            return Ok(false);
        }
        Box::pin(self.lookup_marker(cache_root, namespace, name)).await
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
        let state = self.cached_state(namespace, name);
        if state == Some(MarkerState::Clean)
            || (state.is_none() && self.dir_state.load(Ordering::Acquire) == DIR_ABSENT)
        {
            return Ok(());
        }
        let _write_guard = store.record_write_lock.lock().await;
        remove_marker_if_present(&marker_path(&store.root, namespace, name)).await?;
        self.set_state(namespace, name, MarkerState::Clean);
        Ok(())
    }

    async fn lookup_marker(
        &self,
        cache_root: &Path,
        namespace: &str,
        name: &CanonicalPackageName,
    ) -> Result<bool> {
        let observed_gen = self.generation.load(Ordering::Acquire);
        if self.dir_state.load(Ordering::Acquire) == DIR_UNKNOWN
            && !ensure_dir_present(cache_root, &self.dir_state).await?
        {
            return Ok(self.cached_state(namespace, name) == Some(MarkerState::Purged));
        }
        let state = inspect_package_marker(cache_root, namespace, name).await;
        let effective = self.record_observed_state(namespace, name, observed_gen, state);
        Ok(matches!(effective, MarkerState::Purged))
    }

    fn cached_state(&self, namespace: &str, name: &CanonicalPackageName) -> Option<MarkerState> {
        let entries = self.entries.lock().expect("purged upstream lock poisoned");
        entries
            .get(&(namespace.to_string(), name.clone()))
            .copied()
    }

    fn set_state(&self, namespace: &str, name: &CanonicalPackageName, state: MarkerState) {
        let mut entries = self.entries.lock().expect("purged upstream lock poisoned");
        self.generation.fetch_add(1, Ordering::AcqRel);
        entries.insert((namespace.to_string(), name.clone()), state);
    }

    fn record_observed_state(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        observed_gen: u64,
        state: MarkerState,
    ) -> MarkerState {
        let mut entries = self.entries.lock().expect("purged upstream lock poisoned");
        let key = (namespace.to_string(), name.clone());
        if self.generation.load(Ordering::Acquire) != observed_gen {
            return entries
                .get(&key)
                .copied()
                .unwrap_or(state);
        }
        *entries.entry(key).or_insert(state)
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

async fn inspect_package_marker(
    cache_root: &Path,
    namespace: &str,
    name: &CanonicalPackageName,
) -> MarkerState {
    let path = marker_path(cache_root, namespace, name);
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
    let package_dir = Store::new(cache_root.to_path_buf()).namespaced(namespace).package_dir(name);
    match fs::metadata(&package_dir).await {
        Err(err) if err.kind() == ErrorKind::NotFound => {
            let _ = remove_marker_if_present(&path).await;
            MarkerState::Clean
        }
        Ok(_) | Err(_) => MarkerState::Purged,
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
