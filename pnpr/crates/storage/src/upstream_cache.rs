use super::{BlobWrite, CachedDocument, DocumentStamp, Result, Storage, validate_revision_digest};
use pnpr_package_name::CanonicalPackageName;
use std::time::Duration;
use tokio::fs;

impl Storage {
    pub async fn read_upstream_document(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        ttl: Duration,
    ) -> Result<Option<Vec<u8>>> {
        match self.cached.namespaced(namespace).read_document_entry(name, ttl).await? {
            Some(CachedDocument::Fresh(bytes)) => Ok(Some(bytes)),
            Some(CachedDocument::Stale) | None => Ok(None),
        }
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
        self.cached.namespaced(namespace).write_document(name, bytes).await
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

    pub async fn open_upstream_revision_blob_tmp(
        &self,
        namespace: &str,
        digest: &str,
    ) -> Result<BlobWrite> {
        validate_revision_digest(digest)?;
        self.cached.namespaced(namespace).open_revision_blob_tmp(digest).await
    }

    pub async fn open_upstream_revision_blob(
        &self,
        namespace: &str,
        digest: &str,
    ) -> Result<Option<(fs::File, u64)>> {
        validate_revision_digest(digest)?;
        self.cached.namespaced(namespace).open_revision_blob(digest).await
    }
}
