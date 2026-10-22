mod revision_refs;

mod hosted_backend;

use super::{
    Arc, AsyncReadExt, AsyncSeekExt, BLOBS_DIR, BlobFinalize, BlobWrite, Body, BoxStream,
    CachedDocument, CanonicalPackageName, DOCUMENT_FILE, DocumentStamp, DocumentWrite, Duration,
    ErrorKind, GetRange, HOSTED_REVISION_REF_INDEX_FILE, HOSTED_REVISION_REFS_DIR, HostedBackend,
    HostedBlobFile, HostedDocumentForUpdate, HostedDocumentVersion, HostedRevisionRefIndex,
    HostedRevisionRefWrite, PACKAGES_DIR, Path, PathBuf, RangedBlob, RegistryError, Result,
    SeekFrom, StreamExt, SystemTime, async_trait, create_tmp_file, decode_segment, encode_segment,
    fs, stream, streaming, unique_tmp_path, write_atomic, write_atomic_new,
};

/// One on-disk store rooted at a single directory, laid out as
/// [`crate::layout`] describes.
#[derive(Debug, Clone)]
pub(super) struct Store {
    pub(super) root: PathBuf,
    pub(super) revision_ref_write_lock: Arc<tokio::sync::Mutex<()>>,
    /// Serializes the read-compare-write of a record against every other
    /// write to it, removals included. One process owns this store, so an
    /// in-process lock is the whole of the compare-and-set the object-store
    /// backend gets from an `ETag`.
    pub(super) record_write_lock: Arc<tokio::sync::Mutex<()>>,
}

impl Store {
    pub(super) fn new(root: PathBuf) -> Self {
        Self {
            root,
            revision_ref_write_lock: Arc::new(tokio::sync::Mutex::new(())),
            record_write_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// A disposable store rooted at a sub-path of this one. Used to give a
    /// private `/~<name>/` route its own cache namespace so its documents
    /// and blobs never collide with the public mirror or another upstream.
    pub(super) fn namespaced(&self, prefix: &str) -> Store {
        Store {
            root: self.root.join(prefix),
            revision_ref_write_lock: Arc::clone(&self.revision_ref_write_lock),
            record_write_lock: Arc::clone(&self.record_write_lock),
        }
    }

    pub(super) async fn read_document_entry(
        &self,
        name: &CanonicalPackageName,
        ttl: Duration,
    ) -> Result<Option<CachedDocument>> {
        let path = self.document_path(name);
        let metadata = match fs::metadata(&path).await {
            Ok(m) => m,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let mtime = metadata.modified().map_err(RegistryError::Io)?;
        let age = SystemTime::now().duration_since(mtime).unwrap_or(Duration::ZERO);
        if age <= ttl {
            // Fresh: read the body and serve it.
            Ok(Some(CachedDocument::Fresh(fs::read(&path).await?)))
        } else {
            // Stale: treated as a miss so the caller refetches from the upstream
            // (there is no conditional revalidation), so the body isn't read here.
            Ok(Some(CachedDocument::Stale))
        }
    }

    pub(super) async fn document_stamp(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<DocumentStamp>> {
        match fs::metadata(self.document_path(name)).await {
            Ok(metadata) => Ok(Some(DocumentStamp {
                modified: metadata.modified().map_err(RegistryError::Io)?,
                size: metadata.len(),
            })),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub(super) async fn read_document_any_age(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<Vec<u8>>> {
        let path = self.document_path(name);
        match fs::read(&path).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub(super) async fn write_document(
        &self,
        name: &CanonicalPackageName,
        bytes: &[u8],
    ) -> Result<()> {
        let path = self.document_path(name);
        write_atomic(&path, bytes).await
    }

    pub(super) async fn open_blob(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<Option<(fs::File, u64)>> {
        let path = self.blob_path(name, filename);
        let file = match fs::File::open(&path).await {
            Ok(f) => f,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                let package_dir = self.package_dir(name);
                match fs::metadata(&package_dir).await {
                    Ok(meta) if meta.is_dir() => return Ok(None),
                    Ok(_) => {
                        return Err(std::io::Error::new(
                            ErrorKind::NotADirectory,
                            format!(
                                "package storage path is not a directory: {}",
                                package_dir.display(),
                            ),
                        )
                        .into());
                    }
                    Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
                    Err(err) => return Err(err.into()),
                }
            }
            Err(err) => return Err(err.into()),
        };
        let len = file.metadata().await?.len();
        Ok(Some((file, len)))
    }

    pub(super) async fn open_blob_tmp(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<BlobWrite> {
        let final_path = self.blob_path(name, filename);
        self.open_blob_tmp_at(final_path).await
    }

    pub(super) async fn open_revision_blob(&self, digest: &str) -> Result<Option<(fs::File, u64)>> {
        let file = match fs::File::open(self.revision_blob_path(digest)).await {
            Ok(file) => file,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let len = file.metadata().await?.len();
        Ok(Some((file, len)))
    }

    pub(super) async fn open_revision_blob_tmp(&self, digest: &str) -> Result<BlobWrite> {
        self.open_blob_tmp_at(self.revision_blob_path(digest)).await
    }

    pub(super) async fn open_blob_tmp_at(&self, final_path: PathBuf) -> Result<BlobWrite> {
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let (file, tmp_path) = create_tmp_file(&final_path).await?;
        Ok(BlobWrite { file: Some(file), tmp_path: Some(tmp_path), final_path })
    }

    /// Reserve a tmp path in the destination package directory so the
    /// publish flow can write there and [`Self::finalize_blob`] can
    /// rename within the same directory (atomic on POSIX).
    pub(super) async fn reserve_blob_tmp(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<PathBuf> {
        let final_path = self.blob_path(name, filename);
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        Ok(unique_tmp_path(&final_path))
    }

    pub(super) async fn finalize_blob(
        &self,
        tmp_path: &Path,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<()> {
        let final_path = self.blob_path(name, filename);
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::rename(tmp_path, &final_path).await?;
        Ok(())
    }

    /// Remove the entire package directory. Returns `Ok(false)` if it
    /// didn't exist (treat as a no-op success, matching what verdaccio
    /// does on a duplicate DELETE).
    pub(super) async fn remove_package(&self, name: &CanonicalPackageName) -> Result<bool> {
        let dir = self.package_dir(name);
        match fs::remove_dir_all(&dir).await {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    /// Remove a single blob file. Returns `Ok(false)` when the file
    /// is already gone; the pnpm unpublish flow always issues a DELETE
    /// after the document-update PUT, and a benign 404 here would
    /// surface as a real error to the caller.
    pub(super) async fn remove_blob(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<bool> {
        match fs::remove_file(self.blob_path(name, filename)).await {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    /// Every package with a document, decoded and sorted.
    pub(super) async fn list_package_names(&self) -> Result<Vec<String>> {
        let mut names = Vec::new();
        let Some(mut entries) = read_dir_if_present(&self.root.join(PACKAGES_DIR)).await? else {
            return Ok(names);
        };
        while let Some(entry) = entries.next_entry().await? {
            let segment = entry.file_name();
            let Some(name) = segment.to_str().and_then(decode_segment) else {
                continue;
            };
            if entry.file_type().await?.is_dir()
                && is_regular_file(&entry.path().join(DOCUMENT_FILE)).await?
            {
                names.push(name);
            }
        }
        names.sort();
        Ok(names)
    }

    pub(super) fn package_dir(&self, name: &CanonicalPackageName) -> PathBuf {
        self.root
            .join(PACKAGES_DIR)
            .join(encode_segment(name.as_str()))
    }

    pub(super) fn document_path(&self, name: &CanonicalPackageName) -> PathBuf {
        self.package_dir(name).join(DOCUMENT_FILE)
    }

    pub(super) fn blob_path(&self, name: &CanonicalPackageName, filename: &str) -> PathBuf {
        self.package_dir(name)
            .join(BLOBS_DIR)
            .join(encode_segment(filename))
    }

    /// Keyed by the digest of the revision's tar. `.revisions/sha512` holds
    /// blobs a `.tgz` digest named, which this path never serves.
    pub(super) fn revision_blob_path(&self, digest: &str) -> PathBuf {
        self.root
            .join(".revisions")
            .join("tar-sha512")
            .join(digest)
    }

    /// A record's path. The key's `/` separators become path components, so a
    /// key never rides into a path as one string on a platform that would read
    /// it differently.
    pub(super) fn record_path(&self, namespace: &str, key: &str) -> PathBuf {
        let mut path = self.root.join(namespace);
        path.extend(key.split('/'));
        path
    }

    pub(super) async fn read_record(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        match fs::read(self.record_path(namespace, key)).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub(super) async fn create_record(
        &self,
        namespace: &str,
        key: &str,
        bytes: &[u8],
    ) -> Result<bool> {
        match write_atomic_new(&self.record_path(namespace, key), bytes).await {
            Ok(()) => Ok(true),
            Err(RegistryError::Io(err)) if err.kind() == ErrorKind::AlreadyExists => Ok(false),
            Err(err) => Err(err),
        }
    }

    pub(super) async fn replace_record_if_current(
        &self,
        namespace: &str,
        key: &str,
        expected: &[u8],
        bytes: &[u8],
    ) -> Result<DocumentWrite> {
        let _guard = self.record_write_lock.lock().await;
        if self.read_record(namespace, key).await?.as_deref() != Some(expected) {
            return Ok(DocumentWrite::Conflict);
        }
        write_atomic(&self.record_path(namespace, key), bytes).await?;
        Ok(DocumentWrite::Written)
    }

    /// Under the record-write lock, so a removal cannot land between a
    /// conditional replace's comparison and its write and see the record it
    /// deleted written back.
    pub(super) async fn remove_record(&self, namespace: &str, key: &str) -> Result<bool> {
        let _guard = self.record_write_lock.lock().await;
        match fs::remove_file(self.record_path(namespace, key)).await {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    pub(super) async fn remove_record_if_current(
        &self,
        namespace: &str,
        key: &str,
        expected: &[u8],
    ) -> Result<bool> {
        let _guard = self.record_write_lock.lock().await;
        if self.read_record(namespace, key).await?.as_deref() != Some(expected) {
            return Ok(false);
        }
        match fs::remove_file(self.record_path(namespace, key)).await {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    pub(super) async fn list_record_keys(&self, namespace: &str) -> Result<Vec<String>> {
        let root = self.root.join(namespace);
        let mut keys = Vec::new();
        let mut pending = vec![(root, String::new())];
        while let Some((dir, prefix)) = pending.pop() {
            let Some(mut entries) = read_dir_if_present(&dir).await? else {
                continue;
            };
            while let Some(entry) = entries.next_entry().await? {
                let key = record_key(&prefix, &entry.file_name());
                if entry.file_type().await?.is_dir() {
                    pending.push((entry.path(), key));
                } else {
                    keys.push(key);
                }
            }
        }
        Ok(keys)
    }
}

/// Whether `path` is a regular file, not following a symlink.
pub(super) async fn is_regular_file(path: &Path) -> Result<bool> {
    Ok(symlink_metadata_if_present(path).await?.is_some_and(|metadata| metadata.is_file()))
}

/// Whether `path` is a directory, not following a symlink.
pub(super) async fn is_real_dir(path: &Path) -> Result<bool> {
    Ok(symlink_metadata_if_present(path).await?.is_some_and(|metadata| metadata.is_dir()))
}

async fn symlink_metadata_if_present(path: &Path) -> Result<Option<std::fs::Metadata>> {
    match fs::symlink_metadata(path).await {
        Ok(metadata) => Ok(Some(metadata)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

/// A record key from the walk's directory prefix and one entry name.
pub(super) fn record_key(prefix: &str, name: &std::ffi::OsStr) -> String {
    let name = name.to_string_lossy();
    if prefix.is_empty() { name.into_owned() } else { format!("{prefix}/{name}") }
}

/// The directory's entries, or `None` when the directory does not exist.
pub(crate) async fn read_dir_if_present(dir: &Path) -> Result<Option<fs::ReadDir>> {
    match fs::read_dir(dir).await {
        Ok(entries) => Ok(Some(entries)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}
