pub mod layout;

pub mod metadata;

pub mod publish;

pub mod streaming;

pub mod tombstones;
pub mod transparency_log;

pub mod upload;

pub use bounded_cache::BoundedCache;

pub use atomic_write::{
    remove_atomic_write_temps, unique_tmp_path, write_atomic, write_atomic_new,
};

pub use blob_write::{BlobSlot, BlobWrite, SealedBlob};

pub(crate) use local_store::read_dir_if_present;

pub use object_store::GetRange;

pub(crate) use self::backend::HostedBackend;

pub use self::backend::{HostedDocumentForUpdate, HostedDocumentVersion};

pub use archives::PromotedArchive;
pub use blob_id::{BlobId, parse_blob_key};

pub use registry_records::{RegistryRecord, SCIM_DIRECTORY};

pub use staged_archives::{
    INCOMPLETE_MAX_AGE, STAGED_ARCHIVE_MAX_AGE, STAGED_ARCHIVE_RENEWAL_MARGIN, StagedArchiveKey,
    StagedArchiveUsage,
};

pub use staged_records::PipelineRunKey;

pub use tombstones::{PackageTombstone, TOMBSTONES_NAMESPACE};

pub use layout_state::HostedLayout;

pub use pnpr_package_name::Ecosystem;

pub use tar_integrities::DerivedTar;

mod bounded_cache;
mod tar_integrities;
mod upstream_cache;

mod archives;
mod blob_id;

mod layout_state;
use layout_state::namespace;

mod staged_records;

mod staged_archives;

mod registry_records;

mod atomic_write;
use atomic_write::create_tmp_file;

mod blob_write;

mod tmp_suffix;

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
use pnpr_config::{
    HostedStoreConfig, MetadataBackend, StorageConfig, build_s3_store, normalize_key_prefix,
};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::CanonicalPackageName;
use std::{
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

use layout::{
    ARCHIVES_DIR, BLOBS_DIR, DOCUMENT_FILE, PACKAGES_DIR, blob_segment, package_segments,
};
pub const DOCUMENT_WRITE_RETRIES: usize = 8;
/// Retries a tombstone write allows: higher than the request-path budget
/// because the write has to converge while every replica may be racing it.
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
    /// The whole hosted store: its layout marker and the shared records.
    pub(crate) root: Arc<dyn HostedBackend>,
    /// One namespace of `root`.
    hosted: Arc<dyn HostedBackend>,
    cached: Store,
    /// The hosted backend's tar-integrity namespace, shared by every
    /// organization; see [`Storage::read_tar_integrities`].
    tar_integrities: Arc<dyn HostedBackend>,
    tar_integrity_cache: Arc<tar_integrities::TarIntegrityCache>,
    /// The metadata database every namespace shares.
    database: Arc<metadata::SqlMetadata>,
    /// The metadata of `hosted`'s namespace.
    metadata: metadata::PackageMetadata,
}

/// A partial blob read, including the full size needed for HTTP range headers.
pub enum RangedBlob {
    Read { body: Body, range: std::ops::Range<u64>, size: u64 },
    Unsatisfiable { size: u64 },
}

/// A name stored where a hosted namespace keeps its archives: the key of an
/// archive pnpr stored (see [`parse_blob_key`]), or anything else found
/// there.
#[derive(Debug, Clone)]
pub struct StoredArchive {
    pub name: String,
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
    /// the local `storage` and `cache_storage` roots, with its metadata in
    /// the database `metadata` names. `storage` backs the hosted store when
    /// it's [`HostedStoreConfig::Fs`]; `cache_storage` always backs the proxy
    /// cache and doubles as the S3 backend's local staging scratch.
    pub fn new(
        hosted: &HostedStoreConfig,
        storage: PathBuf,
        cache_storage: PathBuf,
        metadata: &MetadataBackend,
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
        Ok(Self::from_parts(root, cached, metadata::open(metadata)?))
    }

    /// A [`Storage`] over the hosted store `root` and the proxy cache
    /// `cached`, with its metadata in `database`, addressing the default
    /// npm namespace.
    pub(crate) fn from_parts(
        root: Arc<dyn HostedBackend>,
        cached: Store,
        database: Arc<metadata::SqlMetadata>,
    ) -> Self {
        let tar_integrities = root.namespaced(tar_integrities::NAMESPACE);
        let hosted = namespace(&root, "", Ecosystem::Npm);
        let metadata = database.view("", Ecosystem::Npm);
        Self {
            root,
            hosted,
            cached,
            tar_integrities,
            tar_integrity_cache: Arc::default(),
            database,
            metadata,
        }
    }

    /// Build the [`Storage`] `config` describes.
    pub fn from_config(config: &StorageConfig) -> Result<Self> {
        Self::new(
            &config.hosted_backend,
            config.hosted_dir.clone(),
            config.cache_dir.clone(),
            &config.metadata,
        )
    }

    /// Every name stored where this namespace keeps its archives, for
    /// maintenance that compares the store with its metadata.
    #[must_use]
    pub fn hosted_archives(&self) -> BoxStream<'_, Result<StoredArchive>> {
        self.hosted.list_archives()
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
        self.metadata.package_names().await
    }

    // --- Authoritative (hosted) store -----------------------------------

    /// Read the authoritative document for `name`, fresh or stale.
    /// Hosted content has no TTL — it is the source of truth.
    pub async fn read_hosted_document(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<Vec<u8>>> {
        let view = self.metadata.read_view(name).await?;
        Ok(view.map(|view| view.bytes.to_vec()))
    }

    pub async fn read_hosted_document_for_update(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<HostedDocumentForUpdate>> {
        self.metadata.read_for_update(name).await
    }

    /// Write `bytes` as the package's document while the stored one is still
    /// at `version`. `principal` is who asked for the write; the metadata
    /// store records it with every dist-tag the write moves.
    pub async fn write_hosted_document_if_current(
        &self,
        name: &CanonicalPackageName,
        bytes: &[u8],
        version: Option<&HostedDocumentVersion>,
        principal: Option<&str>,
    ) -> Result<DocumentWrite> {
        self.metadata.write_if_current(name, bytes, version, principal).await
    }

    /// Read the hosted document, transform it, and conditionally write it
    /// back under compare-and-swap, retrying on conflict with capped backoff.
    /// `principal` is recorded as for
    /// [`Self::write_hosted_document_if_current`].
    pub async fn update_hosted_document_with_retry<Build>(
        &self,
        name: &CanonicalPackageName,
        retries: usize,
        principal: Option<&str>,
        build: Build,
    ) -> Result<DocumentUpdate>
    where
        Build: FnMut(Option<&[u8]>) -> Result<Option<Vec<u8>>>,
    {
        metadata::update_with_retry(&self.metadata, name, retries, principal, build).await
    }

    /// Where this namespace stages locally: the namespace's directory on the
    /// fs backend, the cache scratch on the S3 backend. In-progress blob
    /// uploads live here.
    #[must_use]
    pub fn hosted_scratch_root(&self) -> &Path {
        self.hosted.local_scratch_root()
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
