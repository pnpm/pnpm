use super::{
    Arc, Body, CanonicalPackageName, DocumentWrite, Ecosystem, HostedBackend,
    HostedDocumentForUpdate, HostedDocumentVersion, ObjectPath, ObjectStoreExt, Path, PathBuf,
    Result, S3Store, async_trait, fs,
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

    async fn check_readiness(&self) -> Result<()> {
        fs::create_dir_all(&self.staging_dir).await
            .map_err(|err| super::RegistryError::StorageUnavailable {
                reason: format!(
                    "s3 staging directory '{}' is not accessible: {err}",
                    self.staging_dir.display(),
                ),
            })?;
        let prefix_path = if self.prefix.is_empty() {
            None
        } else {
            Some(ObjectPath::from(self.prefix.as_str()))
        };
        let check = self.store.list_with_delimiter(prefix_path.as_ref());
        match tokio::time::timeout(std::time::Duration::from_secs(5), check).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(err)) => Err(super::RegistryError::StorageUnavailable {
                reason: format!("S3 store check failed: {err}"),
            }),
            Err(_) => Err(super::RegistryError::StorageUnavailable {
                reason: "S3 store check timed out after 5s".to_string(),
            }),
        }
    }

    async fn read_document(&self, name: &CanonicalPackageName) -> Result<Option<Vec<u8>>> {
        S3Store::read_document(self, name).await
    }

    async fn read_document_for_update(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<HostedDocumentForUpdate>> {
        Ok(S3Store::read_document_for_update(self, name).await?
            .map(|document| HostedDocumentForUpdate {
                bytes: document.bytes,
                version: HostedDocumentVersion::ObjectVersion(document.version),
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
            // The object store never hands one out, so the document it names
            // is not the one stored here.
            Some(HostedDocumentVersion::Generation(_)) => return Ok(DocumentWrite::Conflict),
        };
        if S3Store::write_document_if_current(self, name, bytes, version).await? {
            Ok(DocumentWrite::Written)
        } else {
            Ok(DocumentWrite::Conflict)
        }
    }

    async fn open_archive(&self, key: &str) -> Result<Option<(Body, Option<u64>)>> {
        S3Store::open_archive(self, key).await
    }

    async fn open_archive_range(
        &self,
        key: &str,
        requested: &object_store::GetRange,
    ) -> Result<Option<crate::RangedBlob>> {
        let key = self.archive_key(key);
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

    async fn reserve_archive_tmp(&self) -> Result<PathBuf> {
        self.staging_tmp_path().await
    }

    fn archive_tmp_base(&self) -> PathBuf {
        self.staging_tmp_base()
    }

    async fn put_archive(&self, tmp_path: &Path, key: &str) -> Result<()> {
        let uploaded = self.upload_archive(tmp_path, key).await;
        let _ = fs::remove_file(tmp_path).await;
        uploaded
    }

    async fn remove_archive(&self, key: &str) -> Result<bool> {
        S3Store::remove_archive(self, key).await
    }

    fn list_archives(&self) -> futures_util::stream::BoxStream<'_, Result<crate::StoredArchive>> {
        self.list_archive_objects()
    }

    async fn list_top_level_dirs(&self) -> Result<Vec<String>> {
        S3Store::list_top_level_dirs(self).await
    }

    fn namespaced(&self, segment: &str) -> Arc<dyn HostedBackend> {
        Arc::new(S3Store::namespaced(self, segment))
    }

    fn with_ecosystem(&self, ecosystem: Ecosystem) -> Arc<dyn HostedBackend> {
        Arc::new(S3Store::with_ecosystem(self, ecosystem))
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
