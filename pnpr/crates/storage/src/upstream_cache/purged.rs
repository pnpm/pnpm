use super::{
    CanonicalPackageName, Ecosystem, ErrorKind, Path, PathBuf, Result, Store, fs,
    is_atomic_write_temp_path, read_dir_if_present, write_atomic,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::OnceCell;

/// Subdirectory at the disposable cache root holding durable markers for
/// upstream packages whose authoritative `404` directory removal failed.
pub const PURGED_UPSTREAM_DIR: &str = ".purged";

#[derive(Serialize, Deserialize)]
struct PurgedMarker {
    namespace: String,
    ecosystem: Ecosystem,
    package: String,
}

#[derive(Debug, Default)]
pub(crate) struct PurgedUpstreamPackages {
    loaded: OnceCell<()>,
    has_any: AtomicBool,
    entries: Mutex<HashSet<(String, CanonicalPackageName)>>,
}

impl PurgedUpstreamPackages {
    pub(super) async fn is_purged(
        &self,
        cache_root: &Path,
        namespace: &str,
        name: &CanonicalPackageName,
    ) -> Result<bool> {
        if self.has_any.load(Ordering::Acquire) {
            if self.contains_in_memory(namespace, name) {
                return Ok(true);
            }
            if self.loaded.initialized() {
                return Ok(false);
            }
        } else if self.loaded.initialized() {
            return Ok(false);
        }
        Box::pin(self.ensure_loaded(cache_root)).await?;
        if !self.has_any.load(Ordering::Acquire) {
            return Ok(false);
        }
        Ok(self.contains_in_memory(namespace, name))
    }

    pub(super) async fn mark_purged(
        &self,
        store: &Store,
        namespace: &str,
        name: &CanonicalPackageName,
    ) -> Result<()> {
        let _write_guard = store.record_write_lock.lock().await;
        self.insert_in_memory(namespace, name);
        let _ = Box::pin(self.ensure_loaded(&store.root)).await;
        let package_dir = store.namespaced(namespace).package_dir(name);
        if let Err(err) = fs::metadata(&package_dir).await
            && err.kind() == ErrorKind::NotFound
        {
            self.remove_in_memory(namespace, name);
            return remove_marker_if_present(&marker_path(&store.root, namespace, name)).await;
        }
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
        if self.loaded.initialized() && !self.has_any.load(Ordering::Acquire) {
            return Ok(());
        }
        let _write_guard = store.record_write_lock.lock().await;
        Box::pin(self.ensure_loaded(&store.root)).await?;
        if !self.contains_in_memory(namespace, name) {
            return Ok(());
        }
        remove_marker_if_present(&marker_path(&store.root, namespace, name)).await?;
        self.remove_in_memory(namespace, name);
        Ok(())
    }

    async fn ensure_loaded(&self, cache_root: &Path) -> Result<()> {
        self.loaded
            .get_or_try_init(|| async {
                let loaded = load_markers_from_disk(cache_root).await?;
                if !loaded.is_empty() {
                    let mut entries = self.entries.lock().expect("purged upstream lock poisoned");
                    entries.extend(loaded);
                    self.has_any.store(!entries.is_empty(), Ordering::Release);
                }
                Ok(())
            })
            .await
            .copied()
    }

    fn contains_in_memory(&self, namespace: &str, name: &CanonicalPackageName) -> bool {
        let entries = self.entries.lock().expect("purged upstream lock poisoned");
        entries.contains(&(namespace.to_string(), name.clone()))
    }

    fn insert_in_memory(&self, namespace: &str, name: &CanonicalPackageName) {
        let mut entries = self.entries.lock().expect("purged upstream lock poisoned");
        entries.insert((namespace.to_string(), name.clone()));
        self.has_any.store(true, Ordering::Release);
    }

    fn remove_in_memory(&self, namespace: &str, name: &CanonicalPackageName) {
        let mut entries = self.entries.lock().expect("purged upstream lock poisoned");
        entries.remove(&(namespace.to_string(), name.clone()));
        self.has_any.store(!entries.is_empty(), Ordering::Release);
    }
}

async fn load_markers_from_disk(
    cache_root: &Path,
) -> Result<HashSet<(String, CanonicalPackageName)>> {
    let dir_path = cache_root.join(PURGED_UPSTREAM_DIR);
    let Some(mut dir) = read_dir_if_present(&dir_path).await? else {
        return Ok(HashSet::new());
    };
    let mut loaded = HashSet::new();
    while let Some(entry) = dir.next_entry().await? {
        if let Some(item) = load_single_marker(cache_root, &entry).await? {
            loaded.insert(item);
        }
    }
    Ok(loaded)
}

async fn load_single_marker(
    cache_root: &Path,
    entry: &fs::DirEntry,
) -> Result<Option<(String, CanonicalPackageName)>> {
    let path = entry.path();
    if is_atomic_write_temp_path(&path) {
        return Ok(None);
    }
    match entry.file_type().await {
        Ok(ft) if ft.is_file() => {}
        Ok(_) => return Ok(None),
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    }
    let bytes = match fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let marker: PurgedMarker = serde_json::from_slice(&bytes)?;
    if !is_safe_namespace(&marker.namespace) {
        return Err(
            std::io::Error::new(ErrorKind::InvalidData, "invalid purge marker namespace").into()
        );
    }
    let name = CanonicalPackageName::parse(&marker.package, marker.ecosystem)
        .map_err(|reason| std::io::Error::new(ErrorKind::InvalidData, reason.to_string()))?;
    let package_dir =
        Store::new(cache_root.to_path_buf()).namespaced(&marker.namespace).package_dir(&name);
    match fs::metadata(&package_dir).await {
        Err(err) if err.kind() == ErrorKind::NotFound => {
            let _ = remove_marker_if_present(&path).await;
            Ok(None)
        }
        Ok(_) | Err(_) => Ok(Some((marker.namespace, name))),
    }
}

fn is_safe_namespace(namespace: &str) -> bool {
    !namespace.contains('\\')
        && !namespace.contains('\0')
        && (namespace.is_empty()
            || namespace
                .split('/')
                .all(|seg| !seg.is_empty() && seg != "." && seg != ".."))
}

fn marker_path(cache_root: &Path, namespace: &str, name: &CanonicalPackageName) -> PathBuf {
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
