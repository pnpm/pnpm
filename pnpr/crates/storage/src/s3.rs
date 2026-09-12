//! S3-compatible object-store backend for the **hosted** store.
//!
//! The hosted store is pnpr's source of truth — packages published
//! through its API plus the content served in static mode. When the
//! YAML `s3:` block is present, those authoritative documents and
//! blobs live in an object store instead of on local disk, so the
//! durable data can be replicated by the provider and shared by
//! several stateless pnpr replicas.
//!
//! Any S3-compatible endpoint works: AWS S3 (omit `endpoint`),
//! Cloudflare R2 (`region: auto`, the account endpoint), `MinIO`,
//! Backblaze B2, Wasabi, etc. The disposable proxy cache and the
//! resolver `SQLite` stores stay on local disk regardless —
//! only the hosted store is pluggable.

mod revision_refs;

mod hosted_backend;

use crate::{
    BlobFinalize, DocumentWrite, HOSTED_REVISION_REF_INDEX_FILE, HOSTED_REVISION_REFS_DIR,
    HostedBackend, HostedDocumentForUpdate, HostedDocumentVersion, HostedRevisionRefIndex,
    HostedRevisionRefWrite, wait_after_document_write_conflict,
};
use async_trait::async_trait;
use axum::body::Body;
use futures_util::StreamExt;
use object_store::{
    MultipartUpload, ObjectStore, ObjectStoreExt, PutMode, PutOptions, PutPayload, UpdateVersion,
    path::Path as ObjectPath,
};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::CanonicalPackageName;
use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{fs, io::AsyncReadExt as _};

/// The largest blob sent as one `put`.
///
/// Above this the upload is streamed in parts: a single `put` holds the whole
/// blob in memory, and an image layer runs to gigabytes. The threshold sits
/// above the 100 MiB publish body limit so that every artifact addressed by
/// filename — an npm tarball, a crate, a wheel — still takes the single-put
/// path and keeps its conditional write.
const MAX_SINGLE_PUT_BYTES: u64 = 128 * 1024 * 1024;

/// How much of a large blob is sent per part. S3 requires at least 5 MiB for
/// every part but the last.
const MULTIPART_PART_BYTES: usize = 8 * 1024 * 1024;

const DOCUMENT_FILE: &str = "package.json";
const REVISION_REF_WRITE_RETRIES: usize = 32;

/// Object-store-backed hosted store. Mirrors the verdaccio-shaped
/// key layout the on-disk [`crate`] uses
/// (`<prefix><pkg>/package.json`, `<prefix><pkg>/<basename>.tgz`) so a
/// bucket and a directory hold the same shape.
#[derive(Debug, Clone)]
pub struct S3Store {
    store: Arc<dyn ObjectStore>,
    /// Normalized prefix: empty or `.../`-terminated.
    prefix: String,
    /// Local directory the publish flow stages decoded blobs in
    /// before they're uploaded. The decode/verify step writes through
    /// `std::fs` inside `spawn_blocking`, so it needs a real path even
    /// when the final home is a bucket; a subdirectory of the
    /// proxy-cache root doubles as scratch.
    staging_dir: PathBuf,
    /// The proxy-cache root `staging_dir` sits under. The publish journal
    /// lives here, beside the staged tmp files it rolls forward.
    cache_root: PathBuf,
}

#[derive(Debug)]
pub(crate) struct S3DocumentForUpdate {
    pub(crate) bytes: Vec<u8>,
    pub(crate) version: UpdateVersion,
}

/// Subdirectory of the proxy-cache root where hosted blobs are
/// staged before upload. Its own directory keeps the decode/verify tmp
/// files away from the cache's `<pkg>/` package directories.
const STAGING_SUBDIR: &str = "pnpr-hosted-staging";

/// Send `tmp_path` as parts and complete the upload.
pub(crate) async fn send_parts(tmp_path: &Path, upload: &mut dyn MultipartUpload) -> Result<()> {
    let result = stream_parts(tmp_path, upload).await;
    if result.is_err()
        && let Err(error) = upload.abort().await
    {
        tracing::warn!(%error, "failed to abort multipart upload");
    }
    result
}

async fn stream_parts(tmp_path: &Path, upload: &mut dyn MultipartUpload) -> Result<()> {
    let mut file = fs::File::open(tmp_path).await?;
    let mut part = vec![0u8; MULTIPART_PART_BYTES];
    loop {
        let filled = read_part(&mut file, &mut part).await?;
        if filled == 0 {
            break;
        }
        upload.put_part(PutPayload::from(part[..filled].to_vec())).await?;
    }
    upload.complete().await?;
    Ok(())
}

/// Fill `part` from `file`, returning how much was read. Short of the buffer
/// only at the end of the file, so every part but the last is full.
async fn read_part(file: &mut fs::File, part: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < part.len() {
        let read = file.read(&mut part[filled..]).await?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    Ok(filled)
}

impl S3Store {
    pub fn new(store: Arc<dyn ObjectStore>, prefix: String, cache_root: PathBuf) -> Self {
        Self { store, prefix, staging_dir: cache_root.join(STAGING_SUBDIR), cache_root }
    }

    /// A view of this store with `segment` appended to the key prefix, giving a
    /// hosted registry its own object-key namespace under the same bucket.
    /// Staging scratch is shared (its tmp filenames are already unique).
    #[must_use]
    pub fn namespaced(&self, segment: &str) -> S3Store {
        // An empty segment is the flat root: keep the prefix exactly so it
        // addresses the same object keys as the un-namespaced store, rather than
        // gaining a spurious `/` that points at a different key space.
        if segment.is_empty() {
            return Self {
                store: Arc::clone(&self.store),
                prefix: self.prefix.clone(),
                staging_dir: self.staging_dir.clone(),
                cache_root: self.cache_root.clone(),
            };
        }
        Self {
            store: Arc::clone(&self.store),
            prefix: format!("{}{segment}/", self.prefix),
            staging_dir: self.staging_dir.clone(),
            cache_root: self.cache_root.clone(),
        }
    }

    pub async fn read_document(&self, name: &CanonicalPackageName) -> Result<Option<Vec<u8>>> {
        match self.store.get(&self.document_key(name)).await {
            Ok(result) => Ok(Some(result.bytes().await?.to_vec())),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub(crate) async fn read_document_for_update(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<S3DocumentForUpdate>> {
        match self.store.get(&self.document_key(name)).await {
            Ok(result) => {
                let version = UpdateVersion {
                    e_tag: result.meta.e_tag.clone(),
                    version: result.meta.version.clone(),
                };
                Ok(Some(S3DocumentForUpdate { bytes: result.bytes().await?.to_vec(), version }))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub(crate) async fn write_document_if_current(
        &self,
        name: &CanonicalPackageName,
        bytes: &[u8],
        version: Option<&UpdateVersion>,
    ) -> Result<bool> {
        let mode = match version {
            Some(version) => PutMode::Update(version.clone()),
            None => PutMode::Create,
        };
        match self
            .store
            .put_opts(
                &self.document_key(name),
                PutPayload::from(bytes.to_vec()),
                PutOptions { mode, ..PutOptions::default() },
            )
            .await
        {
            Ok(_) => Ok(true),
            Err(
                object_store::Error::AlreadyExists { .. }
                | object_store::Error::NotFound { .. }
                | object_store::Error::Precondition { .. },
            ) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    /// Open a hosted blob for streaming. `Ok(None)` means the object
    /// doesn't exist so the caller can fall through to the proxy cache
    /// or upstream.
    pub async fn open_blob(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<Option<(Body, Option<u64>)>> {
        match self.store.get(&self.blob_key(name, filename)).await {
            Ok(result) => {
                let len = result.meta.size;
                let stream = result
                    .into_stream()
                    .map(|chunk| chunk.map_err(|err| io::Error::other(err.to_string())));
                Ok(Some((Body::from_stream(stream), Some(len))))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// Reserve a local staging path for the publish flow to decode and
    /// verify a blob into; [`Self::upload_blob`] promotes it to
    /// the bucket once the verification passes.
    pub async fn staging_tmp_path(
        &self,
        _name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<PathBuf> {
        fs::create_dir_all(&self.staging_dir).await?;
        Ok(crate::unique_tmp_path(&self.staging_dir.join(filename)))
    }

    pub async fn upload_blob(
        &self,
        tmp_path: &Path,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<BlobFinalize> {
        let key = self.blob_key(name, filename);
        if fs::metadata(tmp_path).await?.len() > MAX_SINGLE_PUT_BYTES {
            return self.upload_blob_in_parts(tmp_path, &key).await;
        }
        let bytes = fs::read(tmp_path).await?;
        // Create-only. A published version's blob is immutable, so an object
        // already at this key belongs to a concurrent publisher of the same
        // version. Overwriting it would corrupt that artifact against the
        // integrity its document records, so tolerate only byte-identical
        // content and otherwise report a conflict.
        match self
            .store
            .put_opts(
                &key,
                PutPayload::from(bytes),
                PutOptions { mode: PutMode::Create, ..PutOptions::default() },
            )
            .await
        {
            Ok(_) => Ok(BlobFinalize::Written),
            Err(
                object_store::Error::AlreadyExists { .. }
                | object_store::Error::Precondition { .. },
            ) => {
                let ours = fs::read(tmp_path).await?;
                let existing = self.store.get(&key).await?.bytes().await?;
                if existing.as_ref() == ours.as_slice() {
                    Ok(BlobFinalize::AlreadyIdentical)
                } else {
                    Ok(BlobFinalize::Conflict)
                }
            }
            Err(err) => Err(err.into()),
        }
    }

    /// Upload a blob too large to hold in memory, a part at a time.
    ///
    /// Create-only is not available on a multipart upload, and does not need
    /// to be here: only a content-addressed image layer reaches this path, so
    /// a concurrent writer of the same key is writing the same bytes.
    /// Everything addressed by filename stays under
    /// [`MAX_SINGLE_PUT_BYTES`] and keeps the conditional write that makes a
    /// race between two publishers of one version safe.
    async fn upload_blob_in_parts(
        &self,
        tmp_path: &Path,
        key: &ObjectPath,
    ) -> Result<BlobFinalize> {
        let mut upload = self.store.put_multipart(key).await?;
        send_parts(tmp_path, upload.as_mut()).await?;
        Ok(BlobFinalize::Written)
    }

    pub async fn remove_blob(&self, name: &CanonicalPackageName, filename: &str) -> Result<bool> {
        match self.store.delete(&self.blob_key(name, filename)).await {
            Ok(()) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn remove_package(&self, name: &CanonicalPackageName) -> Result<bool> {
        let prefix = ObjectPath::from(format!("{}{}/", self.prefix, name.as_str()));
        let mut listing = self.store.list(Some(&prefix));
        let mut removed = false;
        while let Some(meta) = listing.next().await {
            let meta = meta?;
            self.store.delete(&meta.location).await?;
            removed = true;
        }
        Ok(removed)
    }

    /// List the hosted package names (verdaccio-shaped: a name is a
    /// directory holding a `package.json`). Backs the local search
    /// endpoint when the hosted store lives in a bucket.
    pub async fn list_package_names(&self) -> Result<Vec<String>> {
        let scope = (!self.prefix.is_empty())
            .then(|| ObjectPath::from(self.prefix.trim_end_matches('/').to_string()));
        let mut listing = self.store.list(scope.as_ref());
        let mut names = Vec::new();
        while let Some(meta) = listing.next().await {
            let meta = meta?;
            let key = meta.location.as_ref();
            // Skip anything that isn't actually under our prefix rather
            // than falling back to the full key, which would synthesize
            // a wrong name. (Empty prefix strips to the whole key.)
            let Some(rest) = key.strip_prefix(self.prefix.as_str()) else {
                continue;
            };
            if let Some(name) = rest.strip_suffix(&format!("/{DOCUMENT_FILE}")) {
                names.push(name.to_string());
            }
        }
        Ok(names)
    }

    fn document_key(&self, name: &CanonicalPackageName) -> ObjectPath {
        ObjectPath::from(format!("{}{}/{DOCUMENT_FILE}", self.prefix, name.as_str()))
    }

    fn blob_key(&self, name: &CanonicalPackageName, filename: &str) -> ObjectPath {
        ObjectPath::from(format!("{}{}/{filename}", self.prefix, name.as_str()))
    }

    // Records (see `storage::Storage` for the namespaces and the layout
    // contract shared with the fs backend).

    pub async fn read_record(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        match self.store.get(&self.record_key(namespace, key)).await {
            Ok(result) => Ok(Some(result.bytes().await?.to_vec())),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn create_record(&self, namespace: &str, key: &str, bytes: &[u8]) -> Result<bool> {
        match self
            .store
            .put_opts(
                &self.record_key(namespace, key),
                PutPayload::from(bytes.to_vec()),
                PutOptions { mode: PutMode::Create, ..PutOptions::default() },
            )
            .await
        {
            Ok(_) => Ok(true),
            Err(
                object_store::Error::AlreadyExists { .. }
                | object_store::Error::Precondition { .. },
            ) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    /// Rewrite a record under `If-Match` on the version the caller read, so
    /// only one replica can claim a record whose copy is current. The bytes
    /// are compared as well: a rewrite the caller computed from something
    /// other than what the bucket holds is a conflict even where the version
    /// still matches.
    pub async fn replace_record_if_current(
        &self,
        namespace: &str,
        key: &str,
        expected: &[u8],
        bytes: &[u8],
    ) -> Result<DocumentWrite> {
        let key = self.record_key(namespace, key);
        let result = match self.store.get(&key).await {
            Ok(result) => result,
            // Gone: an approval that finished, or a rejection. A store that
            // failed for any other reason is an error, not a conflict.
            Err(object_store::Error::NotFound { .. }) => return Ok(DocumentWrite::Conflict),
            Err(err) => return Err(err.into()),
        };
        let version = UpdateVersion {
            e_tag: result.meta.e_tag.clone(),
            version: result.meta.version.clone(),
        };
        if result.bytes().await?.as_ref() != expected {
            return Ok(DocumentWrite::Conflict);
        }
        match self
            .store
            .put_opts(
                &key,
                PutPayload::from(bytes.to_vec()),
                PutOptions { mode: PutMode::Update(version), ..PutOptions::default() },
            )
            .await
        {
            Ok(_) => Ok(DocumentWrite::Written),
            Err(
                object_store::Error::AlreadyExists { .. }
                | object_store::Error::NotFound { .. }
                | object_store::Error::Precondition { .. },
            ) => Ok(DocumentWrite::Conflict),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn remove_record(&self, namespace: &str, key: &str) -> Result<bool> {
        match self.store.delete(&self.record_key(namespace, key)).await {
            Ok(()) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn list_record_keys(&self, namespace: &str) -> Result<Vec<String>> {
        let scope = format!("{}{namespace}/", self.prefix);
        let mut listing = self.store.list(Some(&ObjectPath::from(scope.as_str())));
        let mut keys = Vec::new();
        while let Some(meta) = listing.next().await {
            let meta = meta?;
            // `ObjectPath` normalizes what it is built from, so compare
            // against the same normalization rather than the raw prefix.
            let Some(key) =
                meta.location.as_ref().strip_prefix(ObjectPath::from(scope.as_str()).as_ref())
            else {
                continue;
            };
            keys.push(key.trim_start_matches('/').to_string());
        }
        Ok(keys)
    }

    fn record_key(&self, namespace: &str, key: &str) -> ObjectPath {
        ObjectPath::from(format!("{}{namespace}/{key}", self.prefix))
    }
}

#[cfg(test)]
mod tests;
