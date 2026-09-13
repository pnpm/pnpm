use super::{
    BlobWrite, CachedDocument, CanonicalPackageName, Duration, Result, Storage, fs,
    validate_revision_digest,
};
impl Storage {
    /// A fresh cached document for an upstream route, or `None` when it is
    /// absent or older than `ttl`. The upstream path refetches a stale entry
    /// rather than conditionally revalidating it.
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

    /// The cached upstream document regardless of freshness (fresh or stale).
    /// A defensive fallback for an unsolicited upstream `304`: the upstream path
    /// sends no conditional validators, so a `304` means "unchanged" and the
    /// cached body — even past `ttl` — is the right thing to serve rather than
    /// a spurious `404`.
    pub async fn read_upstream_document_any(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
    ) -> Result<Option<Vec<u8>>> {
        // `Duration::MAX` classifies any existing entry as fresh, so its body
        // is returned regardless of age (the stale arm can't be reached here).
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

    /// Purge an upstream's cached entry for `name` — the document and any
    /// cached blobs. Called on a definitive upstream 404: without the
    /// purge, the stale entry would linger past its TTL and a later transient
    /// outage could resurrect the unpublished package through the
    /// stale-if-error fallback.
    pub async fn remove_upstream_package(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
    ) -> Result<bool> {
        self.cached.namespaced(namespace).remove_package(name).await
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
