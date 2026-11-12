use super::{BlobWrite, CachedDocument, DocumentStamp, Result, Storage};
use pnpr_package_name::CanonicalPackageName;
use std::{
    io::ErrorKind,
    time::{Duration, SystemTime},
};
use tokio::fs;

const FETCH_TIME_FILE: &str = "fetched-at.json";

impl Storage {
    pub async fn read_upstream_document(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        ttl: Duration,
    ) -> Result<Option<Vec<u8>>> {
        if self.fresh_upstream_document_stamp(namespace, name, ttl).await?.is_none() {
            return Ok(None);
        }
        self.cached.namespaced(namespace).read_document_any_age(name).await
    }

    /// The content stamp while the upstream fetch, rather than a local rewrite, is fresh.
    pub async fn fresh_upstream_document_stamp(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        ttl: Duration,
    ) -> Result<Option<DocumentStamp>> {
        let Some(stamp) = self.upstream_document_stamp(namespace, name).await? else {
            return Ok(None);
        };
        let store = self.cached.namespaced(namespace);
        let fetched: SystemTime =
            match fs::read(store.package_dir(name).join(FETCH_TIME_FILE)).await {
                Ok(bytes) => serde_json::from_slice(&bytes)?,
                Err(err) if err.kind() == ErrorKind::NotFound => stamp.modified,
                Err(err) => return Err(err.into()),
            };
        Ok((SystemTime::now().duration_since(fetched).unwrap_or(Duration::ZERO) <= ttl).then_some(
            stamp,
        ))
    }

    pub async fn upstream_document_stamp(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
    ) -> Result<Option<DocumentStamp>> {
        self.cached.namespaced(namespace).document_stamp(name).await
    }

    pub async fn read_upstream_document_any(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
    ) -> Result<Option<Vec<u8>>> {
        match self.cached.namespaced(namespace).read_document_entry(name, Duration::MAX).await? {
            Some(CachedDocument::Fresh(bytes)) => Ok(Some(bytes)),
            Some(CachedDocument::Stale) | None => Ok(None),
        }
    }

    pub async fn write_upstream_document(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        bytes: &[u8],
    ) -> Result<()> {
        let store = self.cached.namespaced(namespace);
        store.write_document(name, bytes).await?;
        match fs::remove_file(store.package_dir(name).join(FETCH_TIME_FILE)).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    /// Rewrite local metadata without extending the upstream freshness window.
    /// The caller serializes this with upstream refreshes and removals.
    pub async fn update_upstream_document(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        bytes: &[u8],
    ) -> Result<()> {
        let store = self.cached.namespaced(namespace);
        let stamp = store
            .document_stamp(name)
            .await?
            .ok_or_else(|| {
                std::io::Error::new(
                    ErrorKind::NotFound,
                    format!("cached metadata for {} does not exist", name.as_str()),
                )
            })?;
        let path = store.package_dir(name).join(FETCH_TIME_FILE);
        match super::write_atomic_new(&path, &serde_json::to_vec(&stamp.modified)?).await {
            Ok(()) => {}
            Err(pnpr_error::RegistryError::Io(err)) if err.kind() == ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
        store.write_document(name, bytes).await
    }

    pub async fn remove_upstream_package(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
    ) -> Result<bool> {
        self.cached.namespaced(namespace).remove_package(name).await
    }

    pub async fn remove_upstream_blob(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<bool> {
        self.cached.namespaced(namespace).remove_blob(name, filename).await
    }

    pub async fn open_upstream_blob_tmp(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<BlobWrite> {
        self.cached.namespaced(namespace).open_blob_tmp(name, filename).await
    }

    pub async fn open_upstream_blob(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<Option<(fs::File, u64)>> {
        self.cached.namespaced(namespace).open_blob(name, filename).await
    }
}
