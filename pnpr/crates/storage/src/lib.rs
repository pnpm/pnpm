mod backend;
pub mod journal;
pub mod publish;
mod s3;
pub mod streaming;

use crate::s3::S3Store;
use async_trait::async_trait;
use axum::body::Body;
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
    io::{AsyncSeekExt, AsyncWriteExt},
};

pub(crate) use self::backend::HostedBackend;
pub use self::backend::{BlobFinalize, HostedDocumentForUpdate, HostedDocumentVersion};

const DOCUMENT_FILE: &str = "package.json";
pub(crate) const HOSTED_REVISION_REFS_DIR: &str = ".revisions/sha512";
pub(crate) const HOSTED_REVISION_REF_INDEX_FILE: &str = "index.json";
/// Bounds both the persisted candidate set and work triggered by one digest request.
pub(crate) const MAX_HOSTED_REVISION_REFS: usize = 32;

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct HostedRevisionRefIndex {
    refs: Vec<HostedRevisionRefIndexEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct HostedRevisionRefIndexEntry {
    id: String,
    committed: bool,
    pending_owners: Vec<String>,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostedRevisionRefWrite {
    Claimed,
    AlreadyClaimed,
    Committed,
}

impl HostedRevisionRefIndex {
    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let index: Self = serde_json::from_slice(bytes)?;
        if index.refs.len() > MAX_HOSTED_REVISION_REFS {
            return Err(RegistryError::RevisionReferenceLimit { limit: MAX_HOSTED_REVISION_REFS });
        }
        let mut seen = HashSet::with_capacity(index.refs.len());
        if index.refs.iter().any(|entry| {
            !is_canonical_revision_ref_id(&entry.id)
                || (entry.committed && !entry.pending_owners.is_empty())
                || (!entry.committed && entry.pending_owners.is_empty())
                || entry.pending_owners.iter().enumerate().any(|(owner_index, owner)| {
                    !is_canonical_revision_ref_owner(owner)
                        || entry.pending_owners[..owner_index].contains(owner)
                })
                || !seen.insert(&entry.id)
        }) {
            return Err(RegistryError::Internal {
                reason: "hosted revision reference index is invalid".to_string(),
            });
        }
        Ok(index)
    }

    pub(crate) fn bodies(&self) -> impl Iterator<Item = &[u8]> {
        self.refs.iter().map(|entry| entry.bytes.as_slice())
    }

    pub(crate) fn insert(
        &mut self,
        ref_id: &str,
        owner: &str,
        bytes: &[u8],
    ) -> Result<HostedRevisionRefWrite> {
        if let Some(entry) = self.refs.iter_mut().find(|entry| entry.id == ref_id) {
            if entry.bytes != bytes {
                return Err(RegistryError::Internal {
                    reason: "hosted revision reference body conflicts with its id".to_string(),
                });
            }
            if entry.committed {
                return Ok(HostedRevisionRefWrite::Committed);
            }
            if entry.pending_owners.iter().any(|candidate| candidate == owner) {
                return Ok(HostedRevisionRefWrite::AlreadyClaimed);
            }
            entry.pending_owners.push(owner.to_string());
            return Ok(HostedRevisionRefWrite::Claimed);
        }
        if self.refs.len() == MAX_HOSTED_REVISION_REFS {
            return Err(RegistryError::RevisionReferenceLimit { limit: MAX_HOSTED_REVISION_REFS });
        }
        self.refs.push(HostedRevisionRefIndexEntry {
            id: ref_id.to_string(),
            committed: false,
            pending_owners: vec![owner.to_string()],
            bytes: bytes.to_vec(),
        });
        Ok(HostedRevisionRefWrite::Claimed)
    }

    pub(crate) fn remove_if_owned(&mut self, ref_id: &str, owner: &str) -> bool {
        let Some(entry_index) = self.refs.iter().position(|entry| entry.id == ref_id) else {
            return false;
        };
        let Some(owner_index) =
            self.refs[entry_index].pending_owners.iter().position(|candidate| candidate == owner)
        else {
            return false;
        };
        self.refs[entry_index].pending_owners.remove(owner_index);
        if self.refs[entry_index].pending_owners.is_empty() {
            self.refs.remove(entry_index);
        }
        true
    }

    pub(crate) fn is_owned_by(&self, ref_id: &str, owner: &str) -> bool {
        self.refs.iter().any(|entry| {
            entry.id == ref_id && entry.pending_owners.iter().any(|candidate| candidate == owner)
        })
    }

    pub(crate) fn commit_if_owned(&mut self, ref_id: &str, owner: &str) -> Result<bool> {
        let Some(entry) = self.refs.iter_mut().find(|entry| entry.id == ref_id) else {
            return Err(RegistryError::Internal {
                reason: "hosted revision reference is missing during commit".to_string(),
            });
        };
        if entry.committed {
            return Ok(false);
        }
        if !entry.pending_owners.iter().any(|candidate| candidate == owner) {
            return Err(RegistryError::Internal {
                reason: "hosted revision reference is not owned by its committing transaction"
                    .to_string(),
            });
        }
        entry.committed = true;
        entry.pending_owners.clear();
        Ok(true)
    }

    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("hosted revision reference index serializes")
    }
}

/// Per-process counter feeding [`unique_tmp_path`] so two concurrent
/// writes to the same path don't collide on the same temp filename.
/// Combined with the pid and random suffix, the rename is still atomic
/// on POSIX as long as src and dest sit in the same directory (they do).
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);
const MAX_TEMP_CREATE_ATTEMPTS: usize = 16;
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

/// Handle returned from [`Storage::open_upstream_blob_tmp`]. The caller
/// writes through [`Self::write_all`] (and on success calls [`Self::finalize`] to
/// atomically promote the temp file to the final cache path). The temp
/// path remains armed until promotion succeeds, so cancellation and
/// every error path remove it through [`Drop`].
pub struct BlobWrite {
    file: Option<fs::File>,
    tmp_path: Option<PathBuf>,
    final_path: PathBuf,
}

/// A reserved slot for a hosted-blob write. The publish flow writes
/// the decoded + verified bytes to `tmp_path` (a local file) inside a
/// blocking task, then promotes it to its final home — a rename on the
/// fs backend, an upload on the S3 backend — via
/// [`Storage::finalize_blob_slot`], which recomputes the
/// destination from `name`/`filename`.
#[derive(Debug)]
pub struct BlobSlot {
    pub tmp_path: PathBuf,
    name: CanonicalPackageName,
    filename: String,
}

impl BlobSlot {
    /// Rebuild a slot from its journaled parts so startup recovery can
    /// re-run [`Storage::finalize_blob_slot`] on it.
    pub(crate) fn from_parts(
        tmp_path: PathBuf,
        name: CanonicalPackageName,
        filename: String,
    ) -> Self {
        Self { tmp_path, name, filename }
    }

    pub(crate) fn filename(&self) -> &str {
        &self.filename
    }
}

impl BlobWrite {
    pub async fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.write_all(bytes).await,
            None => Err(std::io::Error::other("blob cache writer is closed")),
        }
    }

    /// Sync the file to disk and rename it to its final cache path.
    pub async fn finalize(mut self) -> std::io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.sync_all().await?,
            None => return Err(std::io::Error::other("blob cache writer is closed")),
        }
        drop(self.file.take());
        if let Some(parent) = self.final_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let tmp_path = self
            .tmp_path
            .as_ref()
            .ok_or_else(|| std::io::Error::other("blob cache temp path is missing"))?;
        fs::rename(tmp_path, &self.final_path).await?;
        self.tmp_path = None;
        Ok(())
    }

    /// Rewind the verified write handle so the caller streams the exact
    /// bytes that were hashed. The handle is opened read+write up front
    /// and reused here — never dropped and reopened by path — so there is
    /// no window for an attacker-writable cache directory to swap the
    /// temp file between verification and streaming.
    pub async fn into_temp_file(mut self) -> std::io::Result<(fs::File, u64, PathBuf)> {
        let Some(mut file) = self.file.take() else {
            return Err(std::io::Error::other("blob cache writer is closed"));
        };
        file.sync_all().await?;
        let len = file.metadata().await?.len();
        let tmp_path = self
            .tmp_path
            .take()
            .ok_or_else(|| std::io::Error::other("blob cache temp path is missing"))?;
        file.seek(SeekFrom::Start(0)).await?;
        Ok((file, len, tmp_path))
    }

    pub async fn abandon(mut self) {
        drop(self.file.take());
        let Some(tmp_path) = self.tmp_path.as_ref() else { return };
        match fs::remove_file(tmp_path).await {
            Ok(()) => self.tmp_path = None,
            Err(err) if err.kind() == ErrorKind::NotFound => self.tmp_path = None,
            Err(_) => {}
        }
    }
}

impl Drop for BlobWrite {
    fn drop(&mut self) {
        drop(self.file.take());
        let Some(tmp_path) = self.tmp_path.take() else { return };
        match std::fs::remove_file(&tmp_path) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => {
                tracing::warn!(?err, path = %tmp_path.display(), "blob cache temp cleanup failed");
            }
        }
    }
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

/// The single-node filesystem backend. It owns its directory tree
/// exclusively, so a document write needs no compare-and-set and a blob
/// is promoted by rename.
#[async_trait]
impl HostedBackend for Store {
    async fn read_document(&self, name: &CanonicalPackageName) -> Result<Option<Vec<u8>>> {
        Store::read_document_any_age(self, name).await
    }

    async fn read_document_for_update(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<HostedDocumentForUpdate>> {
        Ok(Store::read_document_any_age(self, name).await?.map(|bytes| HostedDocumentForUpdate {
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
        Ok(Store::open_blob(self, name, filename)
            .await?
            .map(|(file, len)| (streaming::stream_file(file), Some(len))))
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

    fn local_scratch_root(&self) -> &Path {
        &self.root
    }

    async fn read_staged(&self, object: &str) -> Result<Option<Vec<u8>>> {
        Store::read_staged(self, object).await
    }

    async fn write_staged(&self, object: &str, bytes: &[u8]) -> Result<()> {
        Store::write_staged(self, object, bytes).await
    }

    async fn remove_staged(&self, object: &str) -> Result<bool> {
        Store::remove_staged(self, object).await
    }

    async fn list_staged_ids(&self) -> Result<Vec<String>> {
        Store::list_staged_ids(self).await
    }
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

    /// The hosted package names, used by the local search scan (which
    /// indexes hosted/static packages only, never the proxy mirror).
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

    /// Remove a single blob from both stores. The
    /// partial-unpublish flow calls this after PUT'ing the modified
    /// document back; clearing the proxied mirror too stops
    /// the proxy cache from serving a stale copy of the just-removed
    /// version.
    pub async fn remove_blob(&self, name: &CanonicalPackageName, filename: &str) -> Result<bool> {
        let hosted = self.hosted.remove_blob(name, filename).await?;
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

    pub async fn read_staged_meta(&self, stage_id: &str) -> Result<Option<Vec<u8>>> {
        self.hosted.read_staged(&staged_meta_object(stage_id)?).await
    }

    pub async fn write_staged_meta(&self, stage_id: &str, bytes: &[u8]) -> Result<()> {
        self.hosted.write_staged(&staged_meta_object(stage_id)?, bytes).await
    }

    pub async fn read_staged_body(&self, stage_id: &str) -> Result<Option<Vec<u8>>> {
        self.hosted.read_staged(&staged_body_object(stage_id)?).await
    }

    pub async fn write_staged_body(&self, stage_id: &str, bytes: &[u8]) -> Result<()> {
        self.hosted.write_staged(&staged_body_object(stage_id)?, bytes).await
    }

    /// Remove a staged record — the metadata first, so a concurrent list
    /// never surfaces a record whose body is already gone. `Ok(false)` when
    /// no metadata existed. A body-removal failure is logged rather than
    /// propagated: once the metadata is gone the record is deleted for every
    /// reader, and an error here would misreport that while leaving nothing
    /// for a retry to find (bodies are only discovered through metadata).
    pub async fn remove_staged(&self, stage_id: &str) -> Result<bool> {
        let removed = self.hosted.remove_staged(&staged_meta_object(stage_id)?).await?;
        if let Err(err) = self.hosted.remove_staged(&staged_body_object(stage_id)?).await {
            tracing::warn!(error = %err, stage_id, "staged body cleanup failed after removing its metadata");
        }
        Ok(removed)
    }

    /// Every staged record's id, in unspecified order (the listing endpoint
    /// sorts by staging time).
    pub async fn list_staged_ids(&self) -> Result<Vec<String>> {
        self.hosted.list_staged_ids().await
    }
}

fn validate_revision_digest(digest: &str) -> Result<()> {
    if integrity_addressed_tarball_integrity(digest).is_some() {
        Ok(())
    } else {
        Err(RegistryError::BadRequest { reason: "invalid sha512 revision digest".to_string() })
    }
}

/// Reserved directory (fs) / key segment (S3) holding staged publishes.
/// The leading dot keeps it out of the package namespace: a package name
/// can never start with `.`.
pub(crate) const STAGED_DIR: &str = ".staged";
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

/// One verdaccio-shaped on-disk store rooted at a single directory.
#[derive(Debug, Clone)]
struct Store {
    root: PathBuf,
    revision_ref_write_lock: Arc<tokio::sync::Mutex<()>>,
}

impl Store {
    fn new(root: PathBuf) -> Self {
        Self { root, revision_ref_write_lock: Arc::new(tokio::sync::Mutex::new(())) }
    }

    /// A disposable store rooted at a sub-path of this one. Used to give a
    /// private `/~<name>/` route its own cache namespace so its documents
    /// and blobs never collide with the public mirror or another upstream.
    fn namespaced(&self, prefix: &str) -> Store {
        Store {
            root: self.root.join(prefix),
            revision_ref_write_lock: Arc::clone(&self.revision_ref_write_lock),
        }
    }

    async fn read_document_entry(
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

    async fn read_document_any_age(&self, name: &CanonicalPackageName) -> Result<Option<Vec<u8>>> {
        let path = self.document_path(name);
        match fs::read(&path).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    async fn write_document(&self, name: &CanonicalPackageName, bytes: &[u8]) -> Result<()> {
        let path = self.document_path(name);
        write_atomic(&path, bytes).await
    }

    async fn open_blob(
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

    async fn open_blob_tmp(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<BlobWrite> {
        let final_path = self.blob_path(name, filename);
        self.open_blob_tmp_at(final_path).await
    }

    async fn open_revision_blob(&self, digest: &str) -> Result<Option<(fs::File, u64)>> {
        let file = match fs::File::open(self.revision_blob_path(digest)).await {
            Ok(file) => file,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let len = file.metadata().await?.len();
        Ok(Some((file, len)))
    }

    async fn open_revision_blob_tmp(&self, digest: &str) -> Result<BlobWrite> {
        self.open_blob_tmp_at(self.revision_blob_path(digest)).await
    }

    async fn open_blob_tmp_at(&self, final_path: PathBuf) -> Result<BlobWrite> {
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let (file, tmp_path) = create_tmp_file(&final_path).await?;
        Ok(BlobWrite { file: Some(file), tmp_path: Some(tmp_path), final_path })
    }

    /// Reserve a tmp path in the destination package directory so the
    /// publish flow can write there and [`Self::finalize_blob`] can
    /// rename within the same directory (atomic on POSIX).
    async fn reserve_blob_tmp(
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

    async fn finalize_blob(
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
    async fn remove_package(&self, name: &CanonicalPackageName) -> Result<bool> {
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
    async fn remove_blob(&self, name: &CanonicalPackageName, filename: &str) -> Result<bool> {
        match fs::remove_file(self.blob_path(name, filename)).await {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    /// Walk the storage tree two levels deep to find package names —
    /// directories holding a `package.json`. Layout is
    /// `<root>/<pkg>/package.json` for unscoped and
    /// `<root>/@scope/<name>/package.json` for scoped, so a two-level
    /// walk suffices and avoids descending into blob-adjacent junk.
    /// Hidden entries (the `.pnpr-cache` sibling) are skipped.
    ///
    /// Per-entry stat/read failures are tolerated (the entry is just
    /// skipped) so a single unreadable directory or a stray non-package
    /// file can't fail the whole search — this backs the best-effort,
    /// verdaccio-style `/-/v1/search`, which prefers partial results
    /// over a hard error. A failure to open the store root itself still
    /// propagates.
    async fn list_package_names(&self) -> Result<Vec<String>> {
        let mut names = Vec::new();
        let mut top = match fs::read_dir(&self.root).await {
            Ok(rd) => rd,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(names),
            Err(err) => return Err(err.into()),
        };
        while let Some(entry) = top.next_entry().await? {
            let entry_path = entry.path();
            let entry_name = entry.file_name();
            let name_str = entry_name.to_string_lossy();
            if name_str.starts_with('.') {
                continue;
            }
            if fs::try_exists(entry_path.join(DOCUMENT_FILE)).await.unwrap_or(false) {
                names.push(name_str.into_owned());
                continue;
            }
            if name_str.starts_with('@')
                && let Ok(mut inner) = fs::read_dir(&entry_path).await
            {
                while let Some(child) = inner.next_entry().await? {
                    if fs::try_exists(child.path().join(DOCUMENT_FILE)).await.unwrap_or(false) {
                        names.push(format!("{name_str}/{}", child.file_name().to_string_lossy()));
                    }
                }
            }
        }
        Ok(names)
    }

    async fn read_revision_refs(&self, digest: &str) -> Result<Vec<Vec<u8>>> {
        let index = self.read_revision_ref_index(digest).await?;
        Ok(index.bodies().map(<[u8]>::to_vec).collect())
    }

    async fn write_revision_ref(
        &self,
        digest: &str,
        ref_id: &str,
        owner: &str,
        bytes: &[u8],
    ) -> Result<HostedRevisionRefWrite> {
        let _guard = self.revision_ref_write_lock.lock().await;
        let mut index = self.read_revision_ref_index(digest).await?;
        let outcome = index.insert(ref_id, owner, bytes)?;
        if outcome == HostedRevisionRefWrite::Claimed {
            write_atomic(&self.revision_ref_index_path(digest), &index.to_bytes()).await?;
        }
        Ok(outcome)
    }

    async fn remove_revision_ref(&self, digest: &str, ref_id: &str, owner: &str) -> Result<()> {
        let _guard = self.revision_ref_write_lock.lock().await;
        let mut index = self.read_revision_ref_index(digest).await?;
        if index.remove_if_owned(ref_id, owner) {
            write_atomic(&self.revision_ref_index_path(digest), &index.to_bytes()).await?;
        }
        Ok(())
    }

    async fn commit_revision_ref(&self, digest: &str, ref_id: &str, owner: &str) -> Result<()> {
        let _guard = self.revision_ref_write_lock.lock().await;
        let mut index = self.read_revision_ref_index(digest).await?;
        if index.commit_if_owned(ref_id, owner)? {
            write_atomic(&self.revision_ref_index_path(digest), &index.to_bytes()).await?;
        }
        Ok(())
    }

    async fn read_revision_ref_index(&self, digest: &str) -> Result<HostedRevisionRefIndex> {
        match fs::read(self.revision_ref_index_path(digest)).await {
            Ok(bytes) => HostedRevisionRefIndex::from_bytes(&bytes),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(HostedRevisionRefIndex::default()),
            Err(err) => Err(err.into()),
        }
    }

    fn package_dir(&self, name: &CanonicalPackageName) -> PathBuf {
        self.root.join(name.as_str())
    }

    fn document_path(&self, name: &CanonicalPackageName) -> PathBuf {
        self.package_dir(name).join(DOCUMENT_FILE)
    }

    fn blob_path(&self, name: &CanonicalPackageName, filename: &str) -> PathBuf {
        self.package_dir(name).join(filename)
    }

    fn revision_blob_path(&self, digest: &str) -> PathBuf {
        self.root.join(".revisions").join("sha512").join(digest)
    }

    fn revision_refs_dir(&self, digest: &str) -> PathBuf {
        self.root.join(HOSTED_REVISION_REFS_DIR).join(digest)
    }

    fn revision_ref_index_path(&self, digest: &str) -> PathBuf {
        self.revision_refs_dir(digest).join(HOSTED_REVISION_REF_INDEX_FILE)
    }

    async fn read_staged(&self, object: &str) -> Result<Option<Vec<u8>>> {
        match fs::read(self.root.join(STAGED_DIR).join(object)).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    async fn write_staged(&self, object: &str, bytes: &[u8]) -> Result<()> {
        write_atomic(&self.root.join(STAGED_DIR).join(object), bytes).await
    }

    async fn remove_staged(&self, object: &str) -> Result<bool> {
        match fs::remove_file(self.root.join(STAGED_DIR).join(object)).await {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    async fn list_staged_ids(&self) -> Result<Vec<String>> {
        let mut entries = match fs::read_dir(self.root.join(STAGED_DIR)).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };
        let mut ids = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(id) = staged_id_of_meta_object(&name) {
                ids.push(id.to_string());
            }
        }
        Ok(ids)
    }
}

pub(crate) fn is_canonical_revision_ref_id(ref_id: &str) -> bool {
    ref_id.len() == 64 && ref_id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn is_canonical_revision_ref_owner(owner: &str) -> bool {
    !owner.is_empty()
        && owner.len() <= 64
        && owner.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn validate_revision_ref_id(ref_id: &str) -> Result<()> {
    if is_canonical_revision_ref_id(ref_id) {
        Ok(())
    } else {
        Err(RegistryError::BadRequest { reason: "invalid revision reference id".to_string() })
    }
}

fn validate_revision_ref_owner(owner: &str) -> Result<()> {
    if is_canonical_revision_ref_owner(owner) {
        Ok(())
    } else {
        Err(RegistryError::BadRequest { reason: "invalid revision reference owner".to_string() })
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

pub async fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let (mut file, tmp) = create_tmp_file(path).await?;
    if let Err(err) = file.write_all(bytes).await {
        drop(file);
        let _ = fs::remove_file(&tmp).await;
        return Err(err.into());
    }
    if let Err(err) = file.sync_all().await {
        drop(file);
        let _ = fs::remove_file(&tmp).await;
        return Err(err.into());
    }
    drop(file);
    if let Err(err) = fs::rename(&tmp, path).await {
        let _ = fs::remove_file(&tmp).await;
        return Err(err.into());
    }
    Ok(())
}

pub async fn remove_atomic_write_temps(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let Some(file_name) = path.file_name() else {
        return Ok(());
    };
    let mut prefix = file_name.to_os_string();
    prefix.push(".tmp.");
    let mut entries = match fs::read_dir(parent).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let Some(suffix) = name.as_encoded_bytes().strip_prefix(prefix.as_encoded_bytes()) else {
            continue;
        };
        if !is_atomic_write_temp_suffix(suffix) {
            continue;
        }
        match fs::remove_file(entry.path()).await {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

fn is_atomic_write_temp_suffix(suffix: &[u8]) -> bool {
    let mut parts = suffix.split(|byte| *byte == b'.');
    let (Some(pid), Some(counter), Some(random), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    !pid.is_empty()
        && pid.iter().all(u8::is_ascii_digit)
        && !counter.is_empty()
        && counter.iter().all(u8::is_ascii_digit)
        && random.len() == 16
        && random.iter().all(u8::is_ascii_hexdigit)
}

async fn create_tmp_file(base: &Path) -> Result<(fs::File, PathBuf)> {
    create_tmp_file_with(base, unique_tmp_path).await
}

async fn create_tmp_file_with(
    base: &Path,
    mut next_path: impl FnMut(&Path) -> PathBuf,
) -> Result<(fs::File, PathBuf)> {
    let mut last_already_exists = None;
    for _ in 0..MAX_TEMP_CREATE_ATTEMPTS {
        let tmp_path = next_path(base);
        match fs::OpenOptions::new().read(true).write(true).create_new(true).open(&tmp_path).await {
            Ok(file) => return Ok((file, tmp_path)),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                last_already_exists = Some(err);
            }
            Err(err) => return Err(err.into()),
        }
    }
    Err(last_already_exists
        .unwrap_or_else(|| {
            std::io::Error::new(ErrorKind::AlreadyExists, "temporary path creation collided")
        })
        .into())
}

/// A unique sibling of `base` (`<base>.tmp.<pid>.<counter>.<random>`).
/// Keeping it in `base`'s directory keeps the eventual rename atomic on
/// POSIX. Shared with the S3 backend's staging path.
pub fn unique_tmp_path(base: &Path) -> PathBuf {
    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let mut random = [0u8; 8];
    let random = match getrandom::fill(&mut random) {
        Ok(()) => u64::from_ne_bytes(random),
        Err(_) => 0,
    };
    let mut name = base.file_name().map(std::ffi::OsStr::to_os_string).unwrap_or_default();
    name.push(format!(".tmp.{pid}.{counter}.{random:016x}"));
    match base.parent() {
        Some(parent) => parent.join(name),
        None => PathBuf::from(name),
    }
}

#[cfg(test)]
mod tests;
