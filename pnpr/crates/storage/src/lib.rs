pub mod journal;

pub mod publish;

pub mod streaming;

pub mod upload;

pub use atomic_write::{
    remove_atomic_write_temps, unique_tmp_path, write_atomic, write_atomic_new,
};

pub use revision_ref_index::HostedRevisionRefWrite;

pub(crate) use revision_ref_index::{HostedRevisionRefIndex, is_canonical_revision_ref_owner};

pub use blob_write::{BlobSlot, BlobWrite};

pub(crate) use local_store::read_dir_if_present;

pub use object_store::GetRange;

pub(crate) use self::backend::HostedBackend;

pub use self::backend::{BlobFinalize, HostedDocumentForUpdate, HostedDocumentVersion};

mod staged_records;

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

const DOCUMENT_FILE: &str = "package.json";
/// How deep the hosted walk looks for a package document.
///
/// A name is at most 255 bytes and every component past the first costs at
/// least two of them, so this is the deepest a name can be rather than a
/// policy of its own: the object-store backend applies no depth limit, and a
/// walk that stopped shallower would hide a repository from one backend that
/// the other lists.
const MAX_NAME_COMPONENTS: usize = 128;
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

/// Verdaccio-shaped storage split into two stores with different
/// durability guarantees:
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
/// Both use the same logical layout:
///
/// ```text
/// <root>/
///   <package>/
///     package.json
///     <blob filename>
///   .revisions/sha512/<digest>/
///     index.json
///     <package-version-hash>.json
/// ```
///
/// For scoped packages the package directory is `<root>/@scope/<name>/`.
/// A package's document is always `package.json`; its blobs sit flat
/// beside it (`<basename>-<version>.tgz` on npm) — no `-/` subdirectory.
/// This is the layout `@pnpm/registry-mock` (and verdaccio itself)
/// publishes, so a populated verdaccio storage can be served directly
/// in static mode.
#[derive(Debug, Clone)]
pub struct Storage {
    hosted: Arc<dyn HostedBackend>,
    cached: Store,
}

/// A partial blob read, including the full size needed for HTTP range headers.
pub enum RangedBlob {
    Read { body: Body, range: std::ops::Range<u64>, size: u64 },
    Unsatisfiable { size: u64 },
}

/// A file in the hosted namespace, for offline maintenance.
#[derive(Debug)]
pub struct HostedBlobFile {
    pub path: String,
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
        let hosted: Arc<dyn HostedBackend> = match hosted {
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
        Ok(Self { hosted, cached })
    }

    /// Inventory all regular files, including repositories with no document
    /// and repositories nested below another. Only for offline maintenance.
    #[must_use]
    pub fn hosted_blob_files(&self) -> BoxStream<'_, Result<HostedBlobFile>> {
        self.hosted.list_blob_files()
    }

    /// The hosted package names, used by the local search scan (which
    /// indexes hosted/static packages only, never the proxy mirror).
    /// Build the filesystem listing index for legacy stores before serving requests.
    pub async fn rebuild_package_index(&self) -> Result<()> {
        self.hosted.rebuild_package_index().await
    }

    pub async fn hosted_package_names(&self) -> Result<Vec<String>> {
        self.hosted.list_package_names().await
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

    /// A view whose hosted store is namespaced under `org`, so a hosted
    /// registry's packages live in their own storage namespace — two orgs hosting
    /// the same `name@version` can't collide. The disposable proxy cache is
    /// shared (org registries never touch it). Used by hosted serving and the
    /// org-routed publish flow; the flat (un-namespaced) store remains the
    /// legacy path-less hosted surface.
    #[must_use]
    pub fn for_hosted(&self, org: &str) -> Storage {
        Storage { hosted: self.hosted.namespaced(org), cached: self.cached.clone() }
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

    /// Remove a single blob from both stores. The
    /// partial-unpublish flow calls this after PUT'ing the modified
    /// document back; clearing the proxied mirror too stops
    /// the proxy cache from serving a stale copy of the just-removed
    /// version.
    pub async fn remove_blob(&self, name: &CanonicalPackageName, filename: &str) -> Result<bool> {
        let hosted = self.remove_hosted_blob(name, filename).await?;
        let cached = self.cached.remove_blob(name, filename).await?;
        Ok(hosted || cached)
    }

    /// Remove the package from both stores. Unpublish must purge the
    /// hosted copy *and* any proxied mirror, so a stale cached copy
    /// can't resurface after the package is gone.
    pub async fn remove_package(&self, name: &CanonicalPackageName) -> Result<bool> {
        let hosted = self.hosted.remove_package(name).await?;
        let cached = self.cached.remove_package(name).await?;
        Ok(hosted || cached)
    }

    // --- Per-upstream private cache (the `/~<name>/` registry endpoint) ----
    //
    // A private upstream's documents and blobs are cached under a namespace
    // derived from the upstream and its rotation generation, kept separate from
    // the shared public mirror so they can never be served on the public path
    // or under another upstream. A rotation (new generation) moves to a fresh
    // namespace, so entries fetched with a since-rotated credential age out.

    /// A fresh cached document for an upstream route, or `None` when it is
    /// absent or older than `ttl`. The upstream path refetches a stale entry
    /// rather than conditionally revalidating it.
    pub async fn read_upstream_document(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        ttl: Duration,
    ) -> Result<Option<Vec<u8>>> {
        match self.cached.namespaced(namespace).read_document_entry(name, ttl).await? {
            Some(CachedDocument::Fresh(bytes)) => Ok(Some(bytes)),
            Some(CachedDocument::Stale) | None => Ok(None),
        }
    }

    /// The cached upstream document regardless of freshness (fresh or stale).
    /// A defensive fallback for an unsolicited upstream `304`: the upstream path
    /// sends no conditional validators, so a `304` means "unchanged" and the
    /// cached body — even past `ttl` — is the right thing to serve rather than
    /// a spurious `404`.
    pub async fn read_upstream_document_any(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
    ) -> Result<Option<Vec<u8>>> {
        // `Duration::MAX` classifies any existing entry as fresh, so its body
        // is returned regardless of age (the stale arm can't be reached here).
        match self.cached.namespaced(namespace).read_document_entry(name, Duration::MAX).await? {
            Some(CachedDocument::Fresh(bytes)) => Ok(Some(bytes)),
            Some(CachedDocument::Stale) | None => Ok(None),
        }
    }

    pub async fn write_upstream_document(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        bytes: &[u8],
    ) -> Result<()> {
        self.cached.namespaced(namespace).write_document(name, bytes).await
    }

    /// Purge an upstream's cached entry for `name` — the document and any
    /// cached blobs. Called on a definitive upstream 404: without the
    /// purge, the stale entry would linger past its TTL and a later transient
    /// outage could resurrect the unpublished package through the
    /// stale-if-error fallback.
    pub async fn remove_upstream_package(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
    ) -> Result<bool> {
        self.cached.namespaced(namespace).remove_package(name).await
    }

    pub async fn open_upstream_blob_tmp(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<BlobWrite> {
        self.cached.namespaced(namespace).open_blob_tmp(name, filename).await
    }

    pub async fn open_upstream_blob(
        &self,
        namespace: &str,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<Option<(fs::File, u64)>> {
        self.cached.namespaced(namespace).open_blob(name, filename).await
    }

    pub async fn open_upstream_revision_blob_tmp(
        &self,
        namespace: &str,
        digest: &str,
    ) -> Result<BlobWrite> {
        validate_revision_digest(digest)?;
        self.cached.namespaced(namespace).open_revision_blob_tmp(digest).await
    }

    pub async fn open_upstream_revision_blob(
        &self,
        namespace: &str,
        digest: &str,
    ) -> Result<Option<(fs::File, u64)>> {
        validate_revision_digest(digest)?;
        self.cached.namespaced(namespace).open_revision_blob(digest).await
    }

    /// Promote a tmp blob written by the publish flow to its final
    /// home: a rename on the fs backend, an upload on the S3 backend.
    pub async fn finalize_blob_slot(&self, slot: BlobSlot) -> Result<BlobFinalize> {
        self.hosted.finalize_blob(&slot.tmp_path, &slot.name, &slot.filename).await
    }

    /// Where the hosted backend stages locally: the store root on the fs
    /// backend, the cache scratch on the S3 backend. The publish journal and
    /// in-progress blob uploads both live here.
    #[must_use]
    pub fn hosted_scratch_root(&self) -> &Path {
        self.hosted.local_scratch_root()
    }

    /// The commit journal for this storage's publish flow. It lives in
    /// the same local root as the staged tmp files: the hosted store
    /// root on the fs backend, the cache scratch on the S3 backend
    /// (whose staging paths live there too).
    #[must_use]
    pub fn publish_journal(&self) -> crate::journal::PublishJournal {
        let root = self.hosted.local_scratch_root();
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
    // append-only, keyed `<workspace>/<run id>`.
}

/// Reserved directory (fs) / key segment (S3) holding staged publishes.
/// The leading dot keeps it out of the package namespace: a package name
/// can never start with `.`.
pub(crate) const STAGED_DIR: &str = ".staged";

/// Reserved namespace holding pipeline run records, versioned so a later
/// record shape can live beside this one.
pub(crate) const PIPELINE_RUNS_DIR: &str = ".pipeline-runs/v0";

/// A run's key within its namespace. The identifiers are the client's, so
/// they are checked here as well as by the endpoint that accepts them.
fn pipeline_run_key(workspace: &str, run_id: &str) -> Result<String> {
    Ok(format!("{}/{}", validated_record_name(workspace)?, validated_record_name(run_id)?))
}

/// Reject any identifier that could smuggle a path segment before it reaches
/// a filesystem path or object key.
fn validated_record_name(name: &str) -> Result<&str> {
    let valid = !name.is_empty()
        && !name.starts_with('.')
        && name.chars().all(|char| char.is_ascii_alphanumeric() || matches!(char, '.' | '_' | '-'));
    if valid {
        Ok(name)
    } else {
        Err(RegistryError::BadRequest { reason: format!("invalid record name {name:?}") })
    }
}
const STAGED_META_SUFFIX: &str = ".json";
const STAGED_BODY_SUFFIX: &str = ".body.json";

fn staged_meta_object(stage_id: &str) -> Result<String> {
    Ok(format!("{}{STAGED_META_SUFFIX}", validated_stage_id(stage_id)?))
}

fn staged_body_object(stage_id: &str) -> Result<String> {
    Ok(format!("{}{STAGED_BODY_SUFFIX}", validated_stage_id(stage_id)?))
}

/// Reject any stage id that could smuggle a path segment before it reaches a
/// filesystem path or object key. Handlers validate the UUID shape already;
/// this is the storage layer's own guard.
fn validated_stage_id(stage_id: &str) -> Result<&str> {
    let valid = !stage_id.is_empty()
        && stage_id.chars().all(|char| char.is_ascii_hexdigit() || char == '-');
    if valid {
        Ok(stage_id)
    } else {
        Err(RegistryError::BadRequest { reason: format!("invalid stage id {stage_id:?}") })
    }
}

/// The stage id of a metadata object name, or `None` for anything else in
/// the staged namespace (bodies, tmp files from interrupted writes).
pub(crate) fn staged_id_of_meta_object(object: &str) -> Option<&str> {
    if object.ends_with(STAGED_BODY_SUFFIX) {
        return None;
    }
    object.strip_suffix(STAGED_META_SUFFIX)
}

#[cfg(test)]
mod tests;
