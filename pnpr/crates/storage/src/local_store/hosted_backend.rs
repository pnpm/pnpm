use futures_util::TryStreamExt as _;

use super::{
    ARCHIVES_DIR, Arc, AsyncReadExt, AsyncSeekExt, Body, BoxStream, CanonicalPackageName,
    DocumentWrite, Ecosystem, ErrorKind, GetRange, HostedBackend, HostedDocumentForUpdate,
    HostedDocumentVersion, Path, PathBuf, RangedBlob, Result, SeekFrom, Store, StoredArchive,
    StreamExt, async_trait, fs, read_dir_if_present, stream, streaming, unique_tmp_path,
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

    async fn open_archive(&self, key: &str) -> Result<Option<(Body, Option<u64>)>> {
        Ok(open_file(&self.archive_path(key)).await?
            .map(|(file, len)| (streaming::stream_file(file), Some(len))))
    }

    async fn open_archive_range(&self, key: &str, range: &GetRange) -> Result<Option<RangedBlob>> {
        let Some((mut file, size)) = open_file(&self.archive_path(key)).await? else {
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

    async fn reserve_archive_tmp(&self) -> Result<PathBuf> {
        let base = self.archive_tmp_base();
        if let Some(dir) = base.parent() {
            fs::create_dir_all(dir).await?;
        }
        Ok(unique_tmp_path(&base))
    }

    fn archive_tmp_base(&self) -> PathBuf {
        self.root.join(ARCHIVES_DIR).join("archive")
    }

    async fn put_archive(&self, tmp_path: &Path, key: &str) -> Result<()> {
        let path = self.archive_path(key);
        let stored = async {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::rename(tmp_path, &path).await
        }
        .await;
        if stored.is_err() {
            let _ = fs::remove_file(tmp_path).await;
        }
        Ok(stored?)
    }

    async fn remove_archive(&self, key: &str) -> Result<bool> {
        match fs::remove_file(self.archive_path(key)).await {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    fn list_archives(&self) -> BoxStream<'_, Result<StoredArchive>> {
        let dir = self.root.join(ARCHIVES_DIR);
        stream::once(async move { list_archive_files(&dir).await })
            .map_ok(|files| stream::iter(files.into_iter().map(Ok)))
            .try_flatten()
            .boxed()
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

    fn namespaced(&self, segment: &str) -> Arc<dyn HostedBackend> {
        Arc::new(Store::namespaced(self, segment))
    }

    fn with_ecosystem(&self, ecosystem: Ecosystem) -> Arc<dyn HostedBackend> {
        Arc::new(Store::with_ecosystem(self, ecosystem))
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

    async fn list_record_keys(&self, namespace: &str) -> Result<Vec<String>> {
        Store::list_record_keys(self, namespace).await
    }
}

/// The file at `path` and its length, `None` when there is none.
async fn open_file(path: &Path) -> Result<Option<(fs::File, u64)>> {
    let file = match fs::File::open(path).await {
        Ok(file) => file,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let len = file.metadata().await?.len();
    Ok(Some((file, len)))
}

/// Every file stored in a shard of `dir`, the namespace's archives
/// directory. The staging files beside the shards are not stored archives.
async fn list_archive_files(dir: &Path) -> Result<Vec<StoredArchive>> {
    let mut files = Vec::new();
    let Some(mut shards) = read_dir_if_present(dir).await? else {
        return Ok(files);
    };
    while let Some(shard) = shards.next_entry().await? {
        if shard.file_type().await?.is_dir() {
            let mut entries = fs::read_dir(shard.path()).await?;
            while let Some(entry) = entries.next_entry().await? {
                files.extend(stored_archive(&entry).await?);
            }
        }
    }
    Ok(files)
}

/// The archive `entry` holds, `None` for anything but a file, or for one a
/// reclaim removed while it was listed.
async fn stored_archive(entry: &fs::DirEntry) -> Result<Option<StoredArchive>> {
    let metadata = match entry.metadata().await {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    if !metadata.is_file() {
        return Ok(None);
    }
    Ok(Some(StoredArchive {
        name: entry
            .file_name()
            .to_string_lossy()
            .into_owned(),
        modified: metadata.modified()?,
        size: metadata.len(),
    }))
}
