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

mod records;

use crate::{
    BLOBS_DIR, BlobFinalize, DOCUMENT_FILE, DocumentWrite, HOSTED_REVISION_REF_INDEX_FILE,
    HOSTED_REVISION_REFS_DIR, HostedBackend, HostedDocumentForUpdate, HostedDocumentVersion,
    HostedRevisionRefIndex, HostedRevisionRefWrite, PACKAGES_DIR, decode_segment, encode_segment,
    wait_after_document_write_conflict,
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
        match self.store.put_opts(
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
        match self.store.put_opts(
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
        let prefix = ObjectPath::from(self.package_prefix(name));
        let mut listing = self.store.list(Some(&prefix));
        let mut removed = false;
        while let Some(meta) = listing.next().await {
            let meta = meta?;
            self.store.delete(&meta.location).await?;
            removed = true;
        }
        Ok(removed)
    }

    /// Every hosted package with a document, decoded and sorted. Backs the
    /// local search endpoint when the hosted store lives in a bucket.
    pub async fn list_package_names(&self) -> Result<Vec<String>> {
        let mut listing = self.list_package_files();
        let mut names = Vec::new();
        while let Some(file) = listing.next().await {
            let (name, file, _) = file?;
            if file == PackageFile::Document {
                names.push(name);
            }
        }
        names.sort();
        Ok(names)
    }

    /// Every document and blob below `packages/`, as the package it belongs
    /// to and which of its files it is. Keys the layout does not produce are
    /// left out.
    pub(super) fn list_package_files(
        &self,
    ) -> futures_util::stream::BoxStream<'_, Result<(String, PackageFile, object_store::ObjectMeta)>>
    {
        let scope = ObjectPath::from(format!("{}{PACKAGES_DIR}", self.prefix));
        let normalized_prefix = format!("{}/", scope.as_ref());
        self.store
            .list(Some(&scope))
            .filter_map(move |result| {
                let normalized_prefix = normalized_prefix.clone();
                async move {
                    let meta = match result {
                        Ok(meta) => meta,
                        Err(error) => return Some(Err(error.into())),
                    };
                    let rest = meta.location.as_ref().strip_prefix(normalized_prefix.as_str())?;
                    let (package, file) = rest.split_once('/')?;
                    let name = decode_segment(package)?;
                    let file = if file == DOCUMENT_FILE {
                        PackageFile::Document
                    } else {
                        let blob = file.strip_prefix(BLOBS_DIR)?.strip_prefix('/')?;
                        PackageFile::Blob(decode_segment(blob)?)
                    };
                    Some(Ok((name, file, meta)))
                }
            })
            .boxed()
    }

    pub(super) async fn list_package_blobs(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Vec<String>> {
        let scope = ObjectPath::from(format!("{}{BLOBS_DIR}", self.package_prefix(name)));
        let prefix = format!("{}/", scope.as_ref());
        let mut listing = self.store.list(Some(&scope));
        let mut blobs = Vec::new();
        while let Some(result) = listing.next().await {
            let meta = result?;
            if let Some(rest) = meta.location.as_ref().strip_prefix(prefix.as_str())
                && let Some(decoded) = decode_segment(rest)
            {
                blobs.push(decoded);
            }
        }
        blobs.sort();
        Ok(blobs)
    }

    pub(super) async fn list_top_level_dirs(&self) -> Result<Vec<String>> {
        let scope =
            (!self.prefix.is_empty()).then(|| ObjectPath::from(self.prefix.trim_end_matches('/')));
        let listing = self.store.list_with_delimiter(scope.as_ref()).await?;
        Ok(listing.common_prefixes
            .iter()
            .filter_map(|prefix| prefix.filename())
            .map(|dir| percent_encoding::percent_decode_str(dir).decode_utf8_lossy().into_owned())
            .collect())
    }

    fn package_prefix(&self, name: &CanonicalPackageName) -> String {
        format!("{}{PACKAGES_DIR}/{}/", self.prefix, encode_segment(name.as_str()))
    }

    fn document_key(&self, name: &CanonicalPackageName) -> ObjectPath {
        ObjectPath::from(format!("{}{DOCUMENT_FILE}", self.package_prefix(name)))
    }

    fn blob_key(&self, name: &CanonicalPackageName, filename: &str) -> ObjectPath {
        ObjectPath::from(format!(
            "{}{BLOBS_DIR}/{}",
            self.package_prefix(name),
            encode_segment(filename),
        ))
    }
}

/// Which file of a package an object is.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PackageFile {
    Document,
    Blob(String),
}

#[cfg(test)]
mod tests;
