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

mod hosted_backend;

mod records;

use crate::{
    DOCUMENT_FILE, DocumentWrite, HostedBackend, HostedDocumentForUpdate, HostedDocumentVersion,
    PACKAGES_DIR, layout::ARCHIVES_DIR, package_segments,
};
use async_trait::async_trait;
use axum::body::Body;
use futures_util::StreamExt;
use object_store::{
    MultipartUpload, ObjectStore, ObjectStoreExt, PutMode, PutOptions, PutPayload, UpdateVersion,
    path::Path as ObjectPath,
};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
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

/// Object-store-backed hosted store. Mirrors the verdaccio-shaped
/// key layout the on-disk [`crate`] uses
/// (`<prefix><pkg>/package.json`, `<prefix><pkg>/<basename>.tgz`) so a
/// bucket and a directory hold the same shape.
#[derive(Debug, Clone)]
pub struct S3Store {
    store: Arc<dyn ObjectStore>,
    /// Normalized prefix: empty or `.../`-terminated.
    prefix: String,
    ecosystem: Ecosystem,
    /// Local directory the publish flow stages decoded blobs in
    /// before they're uploaded. The decode/verify step writes through
    /// `std::fs` inside `spawn_blocking`, so it needs a real path even
    /// when the final home is a bucket; a subdirectory of the
    /// proxy-cache root doubles as scratch.
    staging_dir: PathBuf,
    /// The proxy-cache root `staging_dir` sits under, and this backend's local
    /// scratch root.
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
const STAGING_SUBDIR: &str = ".staging";

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
        Self {
            store,
            prefix,
            ecosystem: Ecosystem::Npm,
            staging_dir: cache_root.join(STAGING_SUBDIR),
            cache_root,
        }
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
                ecosystem: self.ecosystem,
                staging_dir: self.staging_dir.clone(),
                cache_root: self.cache_root.clone(),
            };
        }
        Self {
            store: Arc::clone(&self.store),
            prefix: format!("{}{segment}/", self.prefix),
            ecosystem: self.ecosystem,
            staging_dir: self.staging_dir.clone(),
            cache_root: self.cache_root.clone(),
        }
    }

    #[must_use]
    pub fn with_ecosystem(&self, ecosystem: Ecosystem) -> S3Store {
        Self {
            store: Arc::clone(&self.store),
            prefix: self.prefix.clone(),
            ecosystem,
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

    /// Open a hosted archive for streaming, `None` when nothing is stored
    /// under `key`.
    pub async fn open_archive(&self, key: &str) -> Result<Option<(Body, Option<u64>)>> {
        match self.store.get(&self.archive_key(key)).await {
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
    /// verify an archive into; [`Self::upload_archive`] stores it in the
    /// bucket once its identity is known.
    pub async fn staging_tmp_path(&self) -> Result<PathBuf> {
        fs::create_dir_all(&self.staging_dir).await?;
        Ok(crate::unique_tmp_path(&self.staging_tmp_base()))
    }

    /// The path the staging files are named after.
    #[must_use]
    pub fn staging_tmp_base(&self) -> PathBuf {
        self.staging_dir.join("archive")
    }

    /// Upload the staged file at `tmp_path` under `key`. A key only ever
    /// holds the bytes its identity names, so writing over one a concurrent
    /// writer stored writes the same bytes again.
    pub async fn upload_archive(&self, tmp_path: &Path, key: &str) -> Result<()> {
        let key = self.archive_key(key);
        if fs::metadata(tmp_path).await?.len() > MAX_SINGLE_PUT_BYTES {
            let mut upload = self.store.put_multipart(&key).await?;
            return send_parts(tmp_path, upload.as_mut()).await;
        }
        let bytes = fs::read(tmp_path).await?;
        self.store.put(&key, PutPayload::from(bytes)).await?;
        Ok(())
    }

    pub async fn remove_archive(&self, key: &str) -> Result<bool> {
        match self.store.delete(&self.archive_key(key)).await {
            Ok(()) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    /// Every object stored where the namespace keeps its archives.
    pub(super) fn list_archive_objects(
        &self,
    ) -> futures_util::stream::BoxStream<'_, Result<crate::StoredArchive>> {
        let scope = ObjectPath::from(format!("{}{ARCHIVES_DIR}", self.prefix));
        self.store
            .list(Some(&scope))
            .map(|result| {
                let meta = result?;
                Ok(crate::StoredArchive {
                    name: meta.location
                        .filename()
                        .unwrap_or_default()
                        .to_string(),
                    modified: meta.last_modified.into(),
                    size: meta.size,
                })
            })
            .boxed()
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
        let segments = package_segments(name.ecosystem(), name.as_str()).join("/");
        format!("{}{PACKAGES_DIR}/{segments}/", self.prefix)
    }

    fn document_key(&self, name: &CanonicalPackageName) -> ObjectPath {
        ObjectPath::from(format!("{}{DOCUMENT_FILE}", self.package_prefix(name)))
    }

    fn archive_key(&self, key: &str) -> ObjectPath {
        ObjectPath::from(format!("{}{}", self.prefix, crate::layout::archive_object_path(key)))
    }
}

#[cfg(test)]
mod tests;
