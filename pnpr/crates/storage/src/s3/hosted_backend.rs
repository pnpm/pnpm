use super::{
    Arc, BlobFinalize, Body, CanonicalPackageName, DocumentWrite, HostedBackend,
    HostedDocumentForUpdate, HostedDocumentVersion, HostedRevisionRefWrite, ObjectPath,
    ObjectStoreExt, Path, PathBuf, Result, S3Store, StreamExt, async_trait, fs,
};

/// The S3-compatible object-store backend. Document writes are
/// compare-and-set on the object's version, so concurrent publishers on
/// separate nodes cannot lose each other's writes, and a blob is promoted
/// by upload rather than rename.
#[async_trait]
impl HostedBackend for S3Store {
    fn upload_store(&self) -> Option<crate::upload::RemoteUploadStore> {
        Some(crate::upload::RemoteUploadStore::new(
            Arc::clone(&self.store),
            &self.prefix,
            self.cache_root.join(crate::upload::UPLOADS_DIR),
        ))
    }

    async fn read_document(&self, name: &CanonicalPackageName) -> Result<Option<Vec<u8>>> {
        S3Store::read_document(self, name).await
    }

    async fn read_document_for_update(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<HostedDocumentForUpdate>> {
        Ok(S3Store::read_document_for_update(self, name).await?.map(|document| {
            HostedDocumentForUpdate {
                bytes: document.bytes,
                version: HostedDocumentVersion::ObjectVersion(document.version),
            }
        }))
    }

    async fn write_document_if_current(
        &self,
        name: &CanonicalPackageName,
        bytes: &[u8],
        version: Option<&HostedDocumentVersion>,
    ) -> Result<DocumentWrite> {
        let version = match version {
            Some(HostedDocumentVersion::ObjectVersion(version)) => Some(version),
            Some(HostedDocumentVersion::Unversioned) | None => None,
        };
        if S3Store::write_document_if_current(self, name, bytes, version).await? {
            Ok(DocumentWrite::Written)
        } else {
            Ok(DocumentWrite::Conflict)
        }
    }

    async fn open_blob(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<Option<(Body, Option<u64>)>> {
        S3Store::open_blob(self, name, filename).await
    }

    async fn open_blob_range(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
        requested: &object_store::GetRange,
    ) -> Result<Option<crate::RangedBlob>> {
        let key = self.blob_key(name, filename);
        let meta = match self.store.head(&key).await {
            Ok(meta) => meta,
            Err(object_store::Error::NotFound { .. }) => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let size = meta.size;
        let Ok(range) = requested.as_range(size) else {
            return Ok(Some(crate::RangedBlob::Unsatisfiable { size }));
        };
        if range.is_empty() {
            return Ok(Some(crate::RangedBlob::Unsatisfiable { size }));
        }
        let options = object_store::GetOptions {
            range: Some(object_store::GetRange::Bounded(range.clone())),
            if_match: meta.e_tag,
            ..Default::default()
        };
        let result = match self.store.get_opts(&key, options).await {
            Ok(result) => result,
            Err(object_store::Error::NotFound { .. }) => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let body = Body::from_stream(result.into_stream());
        Ok(Some(crate::RangedBlob::Read { body, range, size }))
    }

    async fn reserve_blob_tmp(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<PathBuf> {
        self.staging_tmp_path(name, filename).await
    }

    async fn finalize_blob(
        &self,
        tmp_path: &Path,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<BlobFinalize> {
        let outcome = self.upload_blob(tmp_path, name, filename).await?;
        // Keep the staged tmp on a Conflict so journal roll-forward can
        // re-detect it and exclude the version whose bytes we don't own;
        // once the object is ours there is nothing left to promote.
        if outcome != BlobFinalize::Conflict {
            let _ = fs::remove_file(tmp_path).await;
        }
        Ok(outcome)
    }

    async fn remove_blob(&self, name: &CanonicalPackageName, filename: &str) -> Result<bool> {
        S3Store::remove_blob(self, name, filename).await
    }

    async fn remove_package(&self, name: &CanonicalPackageName) -> Result<bool> {
        S3Store::remove_package(self, name).await
    }

    fn list_blob_files(
        &self,
    ) -> futures_util::stream::BoxStream<'_, Result<crate::HostedBlobFile>> {
        let prefix = ObjectPath::from(self.prefix.trim_end_matches('/'));
        self.store
            .list(Some(&prefix))
            .filter_map(move |result| async move {
                let meta = match result {
                    Ok(meta) => meta,
                    Err(error) => return Some(Err(error.into())),
                };
                let path = meta.location.as_ref().strip_prefix(&self.prefix)?;
                if path.split('/').any(|part| part.starts_with('.')) {
                    return None;
                }
                Some(Ok(crate::HostedBlobFile {
                    path: path.to_string(),
                    modified: meta.last_modified.into(),
                    size: meta.size,
                }))
            })
            .boxed()
    }

    async fn list_package_names(&self) -> Result<Vec<String>> {
        S3Store::list_package_names(self).await
    }

    async fn read_revision_refs(&self, digest: &str) -> Result<Vec<Vec<u8>>> {
        S3Store::read_revision_refs(self, digest).await
    }

    async fn write_revision_ref(
        &self,
        digest: &str,
        ref_id: &str,
        owner: &str,
        bytes: &[u8],
    ) -> Result<HostedRevisionRefWrite> {
        S3Store::write_revision_ref(self, digest, ref_id, owner, bytes).await
    }

    async fn remove_revision_ref(&self, digest: &str, ref_id: &str, owner: &str) -> Result<()> {
        S3Store::remove_revision_ref(self, digest, ref_id, owner).await
    }

    async fn commit_revision_ref(&self, digest: &str, ref_id: &str, owner: &str) -> Result<()> {
        S3Store::commit_revision_ref(self, digest, ref_id, owner).await
    }

    fn namespaced(&self, segment: &str) -> Arc<dyn HostedBackend> {
        Arc::new(S3Store::namespaced(self, segment))
    }

    fn namespace(&self) -> String {
        self.prefix.clone()
    }

    fn local_scratch_root(&self) -> &Path {
        &self.cache_root
    }

    async fn read_record(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        S3Store::read_record(self, namespace, key).await
    }

    async fn create_record(&self, namespace: &str, key: &str, bytes: &[u8]) -> Result<bool> {
        S3Store::create_record(self, namespace, key, bytes).await
    }

    async fn replace_record_if_current(
        &self,
        namespace: &str,
        key: &str,
        expected: &[u8],
        bytes: &[u8],
    ) -> Result<DocumentWrite> {
        S3Store::replace_record_if_current(self, namespace, key, expected, bytes).await
    }

    async fn remove_record(&self, namespace: &str, key: &str) -> Result<bool> {
        S3Store::remove_record(self, namespace, key).await
    }

    async fn list_record_keys(&self, namespace: &str) -> Result<Vec<String>> {
        S3Store::list_record_keys(self, namespace).await
    }
}
