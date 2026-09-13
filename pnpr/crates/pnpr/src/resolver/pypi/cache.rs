use super::{
    AuthHeaders, CachedDocument, IndexReader, MetadataCacheScope, Path, PathBuf, SystemTime,
};
impl IndexReader {
    /// Where `url`'s document is cached. The route scope keys the
    /// namespace, so a private index cached under one caller's credential
    /// is never read back for a caller who does not reproduce that scope.
    /// Where the document read from `url` is cached. `derived_from` is the
    /// digest of the artifact a document was extracted from rather than
    /// read whole, and joins the key so a republished artifact is read
    /// again rather than answered from what came out of the old one.
    pub(super) fn cache_path(
        &self,
        auth: &AuthHeaders,
        url: &url::Url,
        derived_from: Option<&str>,
    ) -> PathBuf {
        let scope = match auth.metadata_scope(url.as_str(), None) {
            MetadataCacheScope::Public => "public".to_string(),
            MetadataCacheScope::Private { descriptor_id } => descriptor_id,
        };
        let key = match derived_from {
            Some(digest) => format!("{url}#{digest}"),
            None => url.to_string(),
        };
        self.cache_dir
            .join(scope)
            .join(format!("{}.json", pnpm_crypto_hash::create_hex_hash(&key)))
    }

    pub(super) async fn cached(&self, path: &Path) -> Option<CachedDocument> {
        let metadata = tokio::fs::metadata(path).await.ok()?;
        let age = SystemTime::now()
            .duration_since(metadata.modified().ok()?)
            .ok()?;
        if age >= self.ttl {
            return None;
        }
        let bytes = tokio::fs::read(path).await.ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Cache a document, best effort: a cache that cannot be written costs
    /// a refetch on the next resolve, which is not worth failing over.
    pub(super) async fn store(path: PathBuf, document: CachedDocument) {
        let _ = tokio::task::spawn_blocking(move || {
            let parent = path.parent()?;
            std::fs::create_dir_all(parent).ok()?;
            let bytes = serde_json::to_vec(&document).ok()?;
            pnpm_fs::write_atomic(&path, &bytes).ok()
        })
        .await;
    }
}
