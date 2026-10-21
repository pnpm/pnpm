pub mod journal;

pub mod layout;

pub mod publish;

pub mod streaming;

pub mod tombstones;

pub mod upload;

pub use atomic_write::{
    remove_atomic_write_temps, unique_tmp_path, write_atomic, write_atomic_new,
};

pub use revision_ref_index::HostedRevisionRefWrite;

pub(crate) use revision_ref_index::{HostedRevisionRefIndex, is_canonical_revision_ref_owner};

pub use blob_write::{BlobSlot, BlobWrite, SealedBlob};

pub(crate) use local_store::read_dir_if_present;

pub use object_store::GetRange;

pub(crate) use self::backend::HostedBackend;

pub use self::backend::{BlobFinalize, HostedDocumentForUpdate, HostedDocumentVersion};

pub use registry_records::{RegistryRecord, SCIM_DIRECTORY};

pub use staged_records::PipelineRunKey;

pub use tombstones::{PackageTombstone, TOMBSTONES_NAMESPACE};

pub use layout_state::HostedLayout;

pub use tar_integrities::DerivedTar;

mod tar_integrities;
mod upstream_cache;

mod layout_state;
use layout_state::namespace;

mod staged_records;

mod registry_records;

mod atomic_write;
use atomic_write::create_tmp_file;

mod revision_ref_index;
use revision_ref_index::{
    validate_revision_digest, validate_revision_ref_id, validate_revision_ref_owner,
};

mod blob_write;

mod local_store;
use local_store::Store;

mod backend;
mod s3;

use crate::s3::S3Store;
use async_trait::async_trait;
use axum::body::Body;
use futures_util::{
    StreamExt,
    stream::{self, BoxStream},
};
use pnpm_crypto_hash::integrity_addressed_tarball_integrity;
use pnpr_config::{HostedStoreConfig, build_s3_store, normalize_key_prefix};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::CanonicalPackageName;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    io::{ErrorKind, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
};

use layout::{BLOBS_DIR, DOCUMENT_FILE, PACKAGES_DIR, decode_segment, encode_segment};
pub(crate) const HOSTED_REVISION_REFS_DIR: &str = ".revisions/sha512";
pub(crate) const HOSTED_REVISION_REF_INDEX_FILE: &str = "index.json";
/// Bounds both the persisted candidate set and work triggered by one digest request.
pub(crate) const MAX_HOSTED_REVISION_REFS: usize = 32;
pub const DOCUMENT_WRITE_RETRIES: usize = 8;
/// Retries the commit path allows the document write: higher than the
/// request-path budget because a sealed transaction has to converge, and a
/// startup recovery may be racing every other replica's recovery at once.
pub(crate) const COMMIT_DOCUMENT_WRITE_RETRIES: usize = 32;
const DOCUMENT_WRITE_CONFLICT_DELAY_MS: u64 = 5;
const MAX_DOCUMENT_WRITE_CONFLICT_DELAY_MS: u64 = 250;

pub(crate) fn document_write_conflict_delay(attempt: usize) -> Duration {
    let delay = DOCUMENT_WRITE_CONFLICT_DELAY_MS
        .saturating_mul(1_u64 << attempt.min(6))
        .min(MAX_DOCUMENT_WRITE_CONFLICT_DELAY_MS);
    Duration::from_millis(delay)
}

pub(crate) async fn wait_after_document_write_conflict(attempt: usize) {
    tokio::time::sleep(document_write_conflict_delay(attempt)).await;
}

/// A cached upstream document, read at a granularity that avoids loading the
/// (potentially multi-MB) body when it isn't needed:
///
/// * `Fresh` — within the TTL; the body is read and ready to serve.
/// * `Stale` — past the TTL. The body is left on disk; a per-registry cache
///   refetches a stale entry rather than revalidating it, so the caller treats
///   `Stale` as a miss.
#[derive(Debug)]
pub enum CachedDocument {
    Fresh(Vec<u8>),
    Stale,
}

/// Storage split into two stores with different durability guarantees:
///
/// * `hosted` — the authoritative source of truth: packages this
///   server hosts directly (published through its API) plus the content
///   served in static mode. Served as-is and never overwritten by an
///   upstream refresh, so a hosted version can't be masked or lost.
///   Backed by a local directory by default, or an S3-compatible
///   object store (S3, Cloudflare R2, `MinIO`, ...) when the YAML `s3:`
///   block is set — see the S3 backend.
/// * `cached` — the disposable mirror of upstream registries. Safe to
///   wipe at any time; it self-heals on the next request. Always local,
///   on scratch/ephemeral disk.
///
/// Both lay out packages as [`layout`] describes. A `Storage` addresses one
/// hosted namespace, the default one unless [`Storage::for_hosted`] picked
/// an org's, and the records every namespace shares.
#[derive(Debug, Clone)]
pub struct Storage {
    /// The whole hosted store: its layout marker, the shared records, and
    /// the publish journal.
    pub(crate) root: Arc<dyn HostedBackend>,
    /// One namespace of `root`.
    hosted: Arc<dyn HostedBackend>,
    cached: Store,
    /// The hosted backend's tar-integrity namespace, shared by every
    /// organization; see [`Storage::read_tar_integrities`].
    tar_integrities: Arc<dyn HostedBackend>,
    tar_integrity_cache: Arc<tar_integrities::TarIntegrityCache>,
}

/// A partial blob read, including the full size needed for HTTP range headers.
pub enum RangedBlob {
    Read { body: Body, range: std::ops::Range<u64>, size: u64 },
    Unsatisfiable { size: u64 },
}

/// A file in the hosted namespace, for offline maintenance.
#[derive(Debug)]
pub struct HostedBlobFile {
    /// The package or repository the file belongs to.
    pub package: String,
    /// The blob's file name, or `None` for the package's document.
    pub blob: Option<String>,
    pub modified: SystemTime,
    pub size: u64,
}

/// When a cached document was written and how large it is, so a value
/// derived from it can tell whether it still describes the document: any
/// rewrite changes the stamp, save one of the same size within a timestamp
/// tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentStamp {
    pub modified: SystemTime,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentWrite {
    Written,
    Conflict,
}

/// Outcome of [`Storage::update_hosted_document_with_retry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentUpdate {
    Written,
    /// `build` returned `Ok(None)`, so nothing was written: the document
    /// the caller wanted to change does not exist, or the change it computed
    /// turned out to be no change at all.
    NotFound,
}

impl Storage {
    /// Build a [`Storage`] from the resolved hosted-store backend plus
    /// the local `storage` and `cache_storage` roots. `storage` backs
    /// the hosted store when it's [`HostedStoreConfig::Fs`];
    /// `cache_storage` always backs the proxy cache and doubles as the
    /// S3 backend's local staging scratch.
    pub fn new(
        hosted: &HostedStoreConfig,
        storage: PathBuf,
        cache_storage: PathBuf,
    ) -> Result<Self> {
        let cached = Store::new(cache_storage.clone());
        let root: Arc<dyn HostedBackend> = match hosted {
            HostedStoreConfig::Fs => Arc::new(Store::new(storage)),
            HostedStoreConfig::S3(settings) => Arc::new(S3Store::new(
                build_s3_store(settings)?,
                settings.normalized_prefix(),
                cache_storage,
            )),
            HostedStoreConfig::ObjectStore { store, prefix } => Arc::new(S3Store::new(
                Arc::clone(store),
                normalize_key_prefix(Some(prefix)),
                cache_storage,
            )),
        };
        let tar_integrities = root.namespaced(tar_integrities::NAMESPACE);
        let hosted = namespace(&root, "");
        Ok(Self { root, hosted, cached, tar_integrities, tar_integrity_cache: Arc::default() })
    }

    /// Inventory all regular files, including repositories with no document
    /// and repositories nested below another. Only for offline maintenance.
    #[must_use]
    pub fn hosted_blob_files(&self) -> BoxStream<'_, Result<HostedBlobFile>> {
        self.hosted.list_blob_files()
    }

    /// Check that the hosted backend and the cache directory are reachable
    /// and accessible for operations.
    pub async fn check_readiness(&self) -> Result<()> {
        self.root.check_readiness().await?;
        fs::create_dir_all(&self.cached.root).await
            .map_err(|err| RegistryError::StorageUnavailable {
                reason: format!(
                    "cache root '{}' is not accessible: {err}",
                    self.cached.root.display(),
                ),
            })?;
        Ok(())
    }

    pub async fn hosted_package_names(&self) -> Result<Vec<String>> {
        self.hosted.list_package_names().await
    }

    pub async fn hosted_package_blobs(
        &self,
        canonical: &CanonicalPackageName,
    ) -> Result<Vec<String>> {
        self.hosted.list_package_blobs(canonical).await
    }

    pub async fn read_hosted_revision_refs(&self, digest: &str) -> Result<Vec<Vec<u8>>> {
        validate_revision_digest(digest)?;
        self.hosted.read_revision_refs(digest).await
    }

    pub async fn write_hosted_revision_ref(
        &self,
        digest: &str,
        ref_id: &str,
        owner: &str,
        bytes: &[u8],
    ) -> Result<HostedRevisionRefWrite> {
        validate_revision_digest(digest)?;
        validate_revision_ref_id(ref_id)?;
        validate_revision_ref_owner(owner)?;
        self.hosted.write_revision_ref(digest, ref_id, owner, bytes).await
    }

    pub(crate) async fn remove_hosted_revision_ref(
        &self,
        digest: &str,
        ref_id: &str,
        owner: &str,
    ) -> Result<()> {
        validate_revision_digest(digest)?;
        validate_revision_ref_id(ref_id)?;
        validate_revision_ref_owner(owner)?;
        self.hosted.remove_revision_ref(digest, ref_id, owner).await
    }

    pub async fn commit_hosted_revision_ref(
        &self,
        digest: &str,
        ref_id: &str,
        owner: &str,
    ) -> Result<()> {
        validate_revision_digest(digest)?;
        validate_revision_ref_id(ref_id)?;
        validate_revision_ref_owner(owner)?;
        self.hosted.commit_revision_ref(digest, ref_id, owner).await
    }

    // --- Authoritative (hosted) store -----------------------------------

    /// Read the authoritative document for `name`, fresh or stale.
    /// Hosted content has no TTL — it is the source of truth.
    pub async fn read_hosted_document(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<Vec<u8>>> {
        self.hosted.read_document(name).await
    }

    pub async fn read_hosted_document_for_update(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<HostedDocumentForUpdate>> {
        self.hosted.read_document_for_update(name).await
    }

    pub async fn write_hosted_document_if_current(
        &self,
        name: &CanonicalPackageName,
        bytes: &[u8],
        version: Option<&HostedDocumentVersion>,
    ) -> Result<DocumentWrite> {
        self.hosted.write_document_if_current(name, bytes, version).await
    }

    /// Read the hosted document, transform it, and conditionally write it
    /// back under compare-and-swap, retrying on conflict with capped backoff.
    ///
    /// `build` receives the current hosted bytes (`None` when the document is
    /// absent) and returns the bytes to write, or `Ok(None)` to abort as
    /// [`DocumentUpdate::NotFound`]; a `build` error aborts without retrying.
    /// After `retries` conflicts the write is surfaced as
    /// [`RegistryError::DocumentWriteConflict`]. Both the dist-tag request
    /// path and journal roll-forward go through here so their conflict handling
    /// stays in one place.
    pub async fn update_hosted_document_with_retry<Build>(
        &self,
        name: &CanonicalPackageName,
        retries: usize,
        mut build: Build,
    ) -> Result<DocumentUpdate>
    where
        Build: FnMut(Option<&[u8]>) -> Result<Option<Vec<u8>>>,
    {
        for attempt in 0..retries {
            let existing = self.read_hosted_document_for_update(name).await?;
            let (existing_bytes, version) = match existing {
                Some(document) => (Some(document.bytes), Some(document.version)),
                None => (None, None),
            };
            let Some(new_bytes) = build(existing_bytes.as_deref())? else {
                return Ok(DocumentUpdate::NotFound);
            };
            match self.write_hosted_document_if_current(name, &new_bytes, version.as_ref()).await? {
                DocumentWrite::Written => return Ok(DocumentUpdate::Written),
                DocumentWrite::Conflict => {
                    if attempt + 1 < retries {
                        wait_after_document_write_conflict(attempt).await;
                    }
                }
            }
        }
        Err(RegistryError::DocumentWriteConflict { package: name.as_str().to_string() })
    }

    /// Open a blob from the authoritative hosted store. Hosted
    /// publish writes verify their SRI before finalization, and static
    /// storage remains operator-controlled rather than an upstream cache.
    pub async fn open_hosted_blob(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<Option<(Body, Option<u64>)>> {
        self.hosted.open_blob(name, filename).await
    }

    /// Open only the requested bytes, without reading the preceding content.
    pub async fn open_hosted_blob_range(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
        range: &GetRange,
    ) -> Result<Option<RangedBlob>> {
        self.hosted.open_blob_range(name, filename, range).await
    }

    /// Reserve a staging slot for a blob this server hosts. The
    /// publish flow streams the decode + hash + write through
    /// `std::fs` inside `spawn_blocking` and only needs the path;
    /// finalize with [`Self::finalize_blob_slot`].
    pub async fn reserve_hosted_blob(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<BlobSlot> {
        let tmp_path = self.hosted.reserve_blob_tmp(name, filename).await?;
        Ok(BlobSlot { tmp_path, name: name.clone(), filename: filename.to_string() })
    }

    /// Remove a hosted blob without changing any proxy-cache namespace.
    pub async fn remove_hosted_blob(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<bool> {
        self.hosted.remove_blob(name, filename).await
    }

    /// Remove a single blob from the hosted store.
    pub async fn remove_blob(&self, name: &CanonicalPackageName, filename: &str) -> Result<bool> {
        self.remove_hosted_blob(name, filename).await
    }

    /// Remove the package from the hosted store.
    pub async fn remove_package(&self, name: &CanonicalPackageName) -> Result<bool> {
        self.hosted.remove_package(name).await
    }

    /// Promote a tmp blob written by the publish flow to its final
    /// home: a rename on the fs backend, an upload on the S3 backend.
    pub async fn finalize_blob_slot(&self, slot: BlobSlot) -> Result<BlobFinalize> {
        self.hosted.finalize_blob(&slot.tmp_path, &slot.name, &slot.filename).await
    }

    /// Where this namespace stages locally: the namespace's directory on the
    /// fs backend, the cache scratch on the S3 backend. In-progress blob
    /// uploads live here.
    #[must_use]
    pub fn hosted_scratch_root(&self) -> &Path {
        self.hosted.local_scratch_root()
    }

    /// The commit journal for this storage's publish flow: in the hosted
    /// store root on the fs backend, the cache scratch on the S3 backend,
    /// whatever namespace this view addresses.
    #[must_use]
    pub fn publish_journal(&self) -> crate::journal::PublishJournal {
        let root = self.root.local_scratch_root();
        crate::journal::PublishJournal::new(root.join(crate::journal::JOURNAL_DIR))
    }

    // --- Staged publishes (`-/stage`) -------------------------------------
    //
    // A staged publish is a publish document held back until it is approved
    // (`POST /-/stage/:id/approve`) or rejected (`DELETE /-/stage/:id`). Each
    // record is two objects in the hosted backend, keyed by the stage id:
    // a small metadata JSON (listed and served as-is) and the full original
    // publish body (replayed through the regular publish flow on approval).
    // Records live under the reserved `.staged/` namespace of the *root*
    // hosted store — never a per-org view — because the stage id is the only
    // thing a later `view`/`approve`/`reject` request carries; the record's
    // metadata remembers which registry the stage was addressed through.

    // --- Pipeline runs (`/-/pnpr/v0/pipeline`) ----------------------------
    //
    // A run record is the account `pnpm pipeline` gave of one run: testimony
    // about something that happened once, not derived data a replica can
    // rebuild, so it lives in the hosted store every replica shares rather
    // than on the replica that happened to receive it. Records are
    // append-only, keyed `<org>/<workspace>/<run id>`.
}

/// Reject any identifier that could smuggle a path segment before it reaches
/// a filesystem path or object key.
fn validated_record_name(name: &str) -> Result<&str> {
    let valid = !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || matches!(char, '.' | '_' | '-'));
    if valid {
        Ok(name)
    } else {
        Err(RegistryError::BadRequest { reason: format!("invalid record name {name:?}") })
    }
}

#[cfg(test)]
mod tests;
