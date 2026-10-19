use futures_util::TryStreamExt as _;

use super::{
    Arc, AsyncReadExt, AsyncSeekExt, BLOBS_DIR, BlobFinalize, Body, BoxStream,
    CanonicalPackageName, DOCUMENT_FILE, DocumentWrite, ErrorKind, GetRange, HostedBackend,
    HostedBlobFile, HostedDocumentForUpdate, HostedDocumentVersion, HostedRevisionRefWrite,
    PACKAGES_DIR, Path, PathBuf, RangedBlob, Result, SeekFrom, Store, StreamExt, async_trait,
    decode_segment, fs, read_dir_if_present, stream, streaming,
};

/// The single-node filesystem backend. It owns its directory tree
/// exclusively, so a document write needs no compare-and-set and a blob
/// is promoted by rename.
#[async_trait]
impl HostedBackend for Store {
    async fn check_readiness(&self) -> Result<()> {
        fs::create_dir_all(&self.root).await
            .map_err(|err| super::RegistryError::StorageUnavailable {
                reason: format!("storage root '{}' is not accessible: {err}", self.root.display()),
            })?;
        Ok(())
    }

    async fn read_document(&self, name: &CanonicalPackageName) -> Result<Option<Vec<u8>>> {
        Store::read_document_any_age(self, name).await
    }

    async fn read_document_for_update(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<HostedDocumentForUpdate>> {
        Ok(Store::read_document_any_age(self, name).await?
            .map(|bytes| HostedDocumentForUpdate {
                bytes,
                version: HostedDocumentVersion::Unversioned,
            }))
    }

    async fn write_document_if_current(
        &self,
        name: &CanonicalPackageName,
        bytes: &[u8],
        _version: Option<&HostedDocumentVersion>,
    ) -> Result<DocumentWrite> {
        Store::write_document(self, name, bytes).await?;
        Ok(DocumentWrite::Written)
    }

    async fn open_blob(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<Option<(Body, Option<u64>)>> {
        Ok(Store::open_blob(self, name, filename).await?
            .map(|(file, len)| (streaming::stream_file(file), Some(len))))
    }

    async fn open_blob_range(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
        range: &GetRange,
    ) -> Result<Option<RangedBlob>> {
        let Some((mut file, size)) = Store::open_blob(self, name, filename).await? else {
            return Ok(None);
        };
        let Ok(range) = range.as_range(size) else {
            return Ok(Some(RangedBlob::Unsatisfiable { size }));
        };
        if range.is_empty() {
            return Ok(Some(RangedBlob::Unsatisfiable { size }));
        }
        file.seek(SeekFrom::Start(range.start)).await?;
        let body = streaming::stream_file(file.take(range.end - range.start));
        Ok(Some(RangedBlob::Read { body, range, size }))
    }

    async fn reserve_blob_tmp(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<PathBuf> {
        Store::reserve_blob_tmp(self, name, filename).await
    }

    async fn finalize_blob(
        &self,
        tmp_path: &Path,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<BlobFinalize> {
        Store::finalize_blob(self, tmp_path, name, filename).await?;
        Ok(BlobFinalize::Written)
    }

    async fn remove_blob(&self, name: &CanonicalPackageName, filename: &str) -> Result<bool> {
        Store::remove_blob(self, name, filename).await
    }

    async fn remove_package(&self, name: &CanonicalPackageName) -> Result<bool> {
        Store::remove_package(self, name).await
    }

    fn list_blob_files(&self) -> BoxStream<'_, Result<HostedBlobFile>> {
        let packages = self.root.join(PACKAGES_DIR);
        stream::once(async move { package_dirs(&packages).await })
            .map_ok(|dirs| {
                stream::iter(dirs)
                    .then(|(name, dir)| package_files(name, dir))
                    .map_ok(|files| stream::iter(files.into_iter().map(Ok)))
            })
            .try_flatten()
            .try_flatten()
            .boxed()
    }

    async fn list_package_names(&self) -> Result<Vec<String>> {
        Store::list_package_names(self).await
    }

    async fn list_package_blobs(&self, name: &CanonicalPackageName) -> Result<Vec<String>> {
        let blobs_dir = self.package_dir(name).join(BLOBS_DIR);
        let Some(mut blobs) = read_dir_if_present(&blobs_dir).await? else {
            return Ok(Vec::new());
        };
        let mut list = Vec::new();
        while let Some(entry) = blobs.next_entry().await? {
            let metadata = entry.metadata().await?;
            if let Some(blob) = entry
                .file_name()
                .to_str()
                .filter(|file| !crate::atomic_write::is_atomic_write_temp_name(file))
                .and_then(decode_segment)
                && metadata.is_file()
            {
                list.push(blob);
            }
        }
        list.sort();
        Ok(list)
    }

    async fn list_top_level_dirs(&self) -> Result<Vec<String>> {
        let mut dirs = Vec::new();
        let Some(mut entries) = read_dir_if_present(&self.root).await? else {
            return Ok(dirs);
        };
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                dirs.push(
                    entry
                        .file_name()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
        Ok(dirs)
    }

    async fn read_revision_refs(&self, digest: &str) -> Result<Vec<Vec<u8>>> {
        Store::read_revision_refs(self, digest).await
    }

    async fn write_revision_ref(
        &self,
        digest: &str,
        ref_id: &str,
        owner: &str,
        bytes: &[u8],
    ) -> Result<HostedRevisionRefWrite> {
        Store::write_revision_ref(self, digest, ref_id, owner, bytes).await
    }

    async fn remove_revision_ref(&self, digest: &str, ref_id: &str, owner: &str) -> Result<()> {
        Store::remove_revision_ref(self, digest, ref_id, owner).await
    }

    async fn commit_revision_ref(&self, digest: &str, ref_id: &str, owner: &str) -> Result<()> {
        Store::commit_revision_ref(self, digest, ref_id, owner).await
    }

    fn namespaced(&self, segment: &str) -> Arc<dyn HostedBackend> {
        Arc::new(Store::namespaced(self, segment))
    }

    fn namespace(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    fn local_scratch_root(&self) -> &Path {
        &self.root
    }

    async fn read_record(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        Store::read_record(self, namespace, key).await
    }

    async fn create_record(&self, namespace: &str, key: &str, bytes: &[u8]) -> Result<bool> {
        Store::create_record(self, namespace, key, bytes).await
    }

    async fn replace_record_if_current(
        &self,
        namespace: &str,
        key: &str,
        expected: &[u8],
        bytes: &[u8],
    ) -> Result<DocumentWrite> {
        Store::replace_record_if_current(self, namespace, key, expected, bytes).await
    }

    async fn remove_record(&self, namespace: &str, key: &str) -> Result<bool> {
        Store::remove_record(self, namespace, key).await
    }

    async fn remove_record_if_current(
        &self,
        namespace: &str,
        key: &str,
        expected: &[u8],
    ) -> Result<bool> {
        Store::remove_record_if_current(self, namespace, key, expected).await
    }

    async fn list_record_keys(&self, namespace: &str) -> Result<Vec<String>> {
        Store::list_record_keys(self, namespace).await
    }
}

/// Every package directory below `packages`, with its decoded name.
async fn package_dirs(packages: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut dirs = Vec::new();
    let Some(mut entries) = read_dir_if_present(packages).await? else {
        return Ok(dirs);
    };
    while let Some(entry) = entries.next_entry().await? {
        // `file_type` does not follow a symlink, so a link cannot lead the
        // inventory, and the deletions that act on it, out of the store.
        if !entry.file_type().await?.is_dir() {
            continue;
        }
        if let Some(name) = entry
            .file_name()
            .to_str()
            .and_then(decode_segment)
        {
            dirs.push((name, entry.path()));
        }
    }
    Ok(dirs)
}

/// The document and blobs of the package `name` in `dir`.
async fn package_files(name: String, dir: PathBuf) -> Result<Vec<HostedBlobFile>> {
    let mut files = Vec::new();
    match fs::symlink_metadata(dir.join(DOCUMENT_FILE)).await {
        Ok(metadata) if metadata.is_file() => files.push(blob_file(&name, None, &metadata)?),
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    let blobs_dir = dir.join(BLOBS_DIR);
    let Some(mut blobs) = read_dir_if_present(&blobs_dir).await? else {
        return Ok(files);
    };
    if !super::is_real_dir(&blobs_dir).await? {
        return Ok(files);
    }
    while let Some(entry) = blobs.next_entry().await? {
        let metadata = entry.metadata().await?;
        // A blob being staged sits beside its final name until it is promoted.
        if let Some(blob) = entry
            .file_name()
            .to_str()
            .filter(|file| !crate::atomic_write::is_atomic_write_temp_name(file))
            .and_then(decode_segment)
            && metadata.is_file()
        {
            files.push(blob_file(&name, Some(blob), &metadata)?);
        }
    }
    Ok(files)
}

fn blob_file(
    package: &str,
    blob: Option<String>,
    metadata: &std::fs::Metadata,
) -> Result<HostedBlobFile> {
    Ok(HostedBlobFile {
        package: package.to_string(),
        blob,
        modified: metadata.modified()?,
        size: metadata.len(),
    })
}
