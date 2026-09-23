use super::{
    Arc, AsyncReadExt, AsyncSeekExt, BlobFinalize, Body, BoxStream, CanonicalPackageName,
    DocumentWrite, ErrorKind, GetRange, HostedBackend, HostedBlobFile, HostedDocumentForUpdate,
    HostedDocumentVersion, HostedRevisionRefWrite, Path, PathBuf, RangedBlob, Result, SeekFrom,
    Store, StreamExt, async_trait, create_tmp_file, fs, read_dir_if_present, stream, streaming,
    write_atomic,
};

/// The single-node filesystem backend. It owns its directory tree
/// exclusively, so a document write needs no compare-and-set and a blob
/// is promoted by rename.
#[async_trait]
impl HostedBackend for Store {
    async fn rebuild_package_index(&self) -> Result<()> {
        let complete = self.root.join(".package-index/.complete");
        if fs::try_exists(&complete).await? {
            return Ok(());
        }
        let index = self.root.join(".package-index");
        let mut files = self.list_blob_files();
        while let Some(file) = files.next().await {
            let file = file?;
            let Some(name) = file.path.strip_suffix("/package.json") else { continue };
            let marker_dir = index.join(name);
            fs::create_dir_all(&marker_dir).await?;
            write_index_marker(&marker_dir.join(".present")).await?;
        }
        write_atomic(&complete, b"").await
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
        let marker = self.root
            .join(".package-index")
            .join(name.as_str())
            .join(".present");
        let indexed = fs::try_exists(&marker).await?;
        write_atomic(&marker, b"").await?;
        if let Err(err) = Store::write_document(self, name, bytes).await {
            if !indexed {
                fs::remove_file(&marker).await?;
                self.prune_package_index(name).await?;
            }
            return Err(err);
        }
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
        let removed = Store::remove_package(self, name).await?;
        match fs::remove_dir_all(self.root.join(".package-index").join(name.as_str())).await {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
        self.prune_package_index(name).await?;
        Ok(removed)
    }

    fn list_blob_files(&self) -> BoxStream<'_, Result<HostedBlobFile>> {
        let root = &self.root;
        stream::try_unfold(
            (true, Vec::<fs::ReadDir>::new()),
            move |(first, mut directories)| async move {
                if first {
                    let Some(entries) = read_dir_if_present(root).await? else {
                        return Ok(None);
                    };
                    directories.push(entries);
                }
                let Some(file) = next_blob_file(root, &mut directories).await? else {
                    return Ok(None);
                };
                Ok(Some((file, (false, directories))))
            },
        )
        .boxed()
    }

    async fn list_package_names(&self) -> Result<Vec<String>> {
        Store::list_package_names(self).await
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

    async fn list_record_keys(&self, namespace: &str) -> Result<Vec<String>> {
        Store::list_record_keys(self, namespace).await
    }
}

/// The next file below `root`, walking the directory stack depth-first.
async fn next_blob_file(
    root: &Path,
    directories: &mut Vec<fs::ReadDir>,
) -> Result<Option<HostedBlobFile>> {
    while let Some(entries) = directories.last_mut() {
        let Some(entry) = entries.next_entry().await? else {
            directories.pop();
            continue;
        };
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with('.')
        {
            continue;
        }
        let kind = entry.file_type().await?;
        if kind.is_dir() {
            directories.push(fs::read_dir(entry.path()).await?);
        } else if kind.is_file() {
            return blob_file(root, &entry).await.map(Some);
        }
    }
    Ok(None)
}

async fn blob_file(root: &Path, entry: &fs::DirEntry) -> Result<HostedBlobFile> {
    let metadata = entry.metadata().await?;
    let path = entry
        .path()
        .strip_prefix(root)
        .expect("entry is below the store root")
        .to_string_lossy()
        .replace('\\', "/");
    Ok(HostedBlobFile { path, modified: metadata.modified()?, size: metadata.len() })
}

/// Publishes an empty index marker the way `write_atomic` does, by renaming
/// a temporary sibling over it, so a symlink planted at the marker path is
/// replaced rather than followed. Only the flush differs: on Apple platforms
/// `sync_all` is `F_FULLFSYNC`, which also flushes the drive cache and costs
/// about 4 ms alone and 19 ms under load, once per package. A plain `fsync`
/// hands the marker to the device, and the `F_FULLFSYNC` that publishes
/// `.complete` afterwards flushes the drive cache for everything written
/// before it.
async fn write_index_marker(path: &Path) -> Result<()> {
    let (file, tmp) = create_tmp_file(path).await?;
    let published = match flush_to_device(file).await {
        Ok(()) => fs::rename(&tmp, path).await,
        Err(err) => Err(err),
    };
    if let Err(err) = published {
        let _ = fs::remove_file(&tmp).await;
        return Err(err.into());
    }
    Ok(())
}

/// `fsync` without the drive-cache flush that `sync_all` adds on Apple
/// platforms; `sync_all` everywhere else.
async fn flush_to_device(file: fs::File) -> std::io::Result<()> {
    #[cfg(target_vendor = "apple")]
    {
        let file = file.into_std().await;
        tokio::task::spawn_blocking(move || {
            use std::os::fd::AsRawFd as _;
            // SAFETY: `file` keeps the descriptor open for the whole call.
            if unsafe { libc::fsync(file.as_raw_fd()) } == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        })
        .await
        .map_err(std::io::Error::other)?
    }
    #[cfg(not(target_vendor = "apple"))]
    file.sync_all().await
}
