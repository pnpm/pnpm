use crate::{
    config::HostedStoreConfig,
    error::{RegistryError, Result},
    package_name::PackageName,
    s3::S3Store,
    streaming,
};
use axum::body::Body;
use object_store::UpdateVersion;
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
    io::{AsyncSeekExt, AsyncWriteExt},
};

const PACKUMENT_FILE: &str = "package.json";

/// Per-process counter feeding [`unique_tmp_path`] so two concurrent
/// writes to the same path don't collide on the same temp filename.
/// Combined with the pid and random suffix, the rename is still atomic
/// on POSIX as long as src and dest sit in the same directory (they do).
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);
const MAX_TEMP_CREATE_ATTEMPTS: usize = 16;
pub(crate) const PACKUMENT_WRITE_RETRIES: usize = 8;
pub(crate) const RECOVERY_PACKUMENT_WRITE_RETRIES: usize = 32;
const PACKUMENT_WRITE_CONFLICT_DELAY_MS: u64 = 5;
const MAX_PACKUMENT_WRITE_CONFLICT_DELAY_MS: u64 = 250;

pub(crate) fn packument_write_conflict_delay(attempt: usize) -> Duration {
    let delay = PACKUMENT_WRITE_CONFLICT_DELAY_MS
        .saturating_mul(1_u64 << attempt.min(6))
        .min(MAX_PACKUMENT_WRITE_CONFLICT_DELAY_MS);
    Duration::from_millis(delay)
}

pub(crate) async fn wait_after_packument_write_conflict(attempt: usize) {
    tokio::time::sleep(packument_write_conflict_delay(attempt)).await;
}

/// Handle returned from [`Storage::open_upstream_tarball_tmp`]. The caller
/// writes through [`Self::write_all`] (and on success calls [`Self::finalize`] to
/// atomically promote the temp file to the final cache path). The temp
/// path remains armed until promotion succeeds, so cancellation and
/// every error path remove it through [`Drop`].
pub struct TarballWrite {
    file: Option<fs::File>,
    tmp_path: Option<PathBuf>,
    final_path: PathBuf,
}

/// A reserved slot for a hosted-tarball write. The publish flow writes
/// the decoded + verified tarball to `tmp_path` (a local file) inside a
/// blocking task, then promotes it to its final home — a rename on the
/// fs backend, an upload on the S3 backend — via
/// [`Storage::finalize_tarball_slot`], which recomputes the
/// destination from `name`/`filename`.
#[derive(Debug)]
pub struct TarballSlot {
    pub tmp_path: PathBuf,
    name: PackageName,
    filename: String,
}

impl TarballSlot {
    /// Rebuild a slot from its journaled parts so startup recovery can
    /// re-run [`Storage::finalize_tarball_slot`] on it.
    pub(crate) fn from_parts(tmp_path: PathBuf, name: PackageName, filename: String) -> Self {
        Self { tmp_path, name, filename }
    }

    pub(crate) fn filename(&self) -> &str {
        &self.filename
    }
}

impl TarballWrite {
    pub async fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.write_all(bytes).await,
            None => Err(std::io::Error::other("tarball cache writer is closed")),
        }
    }

    /// Sync the file to disk and rename it to its final cache path.
    pub async fn finalize(mut self) -> std::io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.sync_all().await?,
            None => return Err(std::io::Error::other("tarball cache writer is closed")),
        }
        drop(self.file.take());
        if let Some(parent) = self.final_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let tmp_path = self
            .tmp_path
            .as_ref()
            .ok_or_else(|| std::io::Error::other("tarball cache temp path is missing"))?;
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
            return Err(std::io::Error::other("tarball cache writer is closed"));
        };
        file.sync_all().await?;
        let len = file.metadata().await?.len();
        let tmp_path = self
            .tmp_path
            .take()
            .ok_or_else(|| std::io::Error::other("tarball cache temp path is missing"))?;
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

impl Drop for TarballWrite {
    fn drop(&mut self) {
        drop(self.file.take());
        let Some(tmp_path) = self.tmp_path.take() else { return };
        match std::fs::remove_file(&tmp_path) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => {
                tracing::warn!(?err, path = %tmp_path.display(), "tarball cache temp cleanup failed");
            }
        }
    }
}

/// A cached upstream packument, read at a granularity that avoids loading the
/// (potentially multi-MB) body when it isn't needed:
///
/// * `Fresh` — within the TTL; the body is read and ready to serve.
/// * `Stale` — past the TTL. The body is left on disk; a per-registry cache
///   refetches a stale entry rather than revalidating it, so the caller treats
///   `Stale` as a miss.
#[derive(Debug)]
pub enum CachedPackument {
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
///   block is set — see [`crate::s3`].
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
///     <basename>-<version>.tgz
/// ```
///
/// For scoped packages the package directory is `<root>/@scope/<name>/`.
/// Tarballs sit flat alongside `package.json` — no `-/` subdirectory.
/// This is the layout `@pnpm/registry-mock` (and verdaccio itself)
/// publishes, so a populated verdaccio storage can be served directly
/// in static mode.
#[derive(Debug, Clone)]
pub struct Storage {
    hosted: HostedStore,
    cached: Store,
}

/// The hosted store's pluggable backend: a local directory or an
/// S3-compatible bucket. The disposable `cached` store is always
/// local, so only the hosted side varies.
#[derive(Debug, Clone)]
enum HostedStore {
    Fs(Store),
    S3(S3Store),
}

#[derive(Debug)]
pub(crate) struct HostedPackumentForUpdate {
    pub(crate) bytes: Vec<u8>,
    pub(crate) version: HostedPackumentVersion,
}

#[derive(Debug)]
pub(crate) enum HostedPackumentVersion {
    Fs,
    S3(UpdateVersion),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackumentWrite {
    Written,
    Conflict,
}

/// Outcome of [`Storage::update_hosted_packument_with_retry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackumentUpdate {
    Written,
    /// The `build` closure reported that the packument does not exist
    /// (returned `Ok(None)`), so there was nothing to update.
    NotFound,
}

/// Outcome of promoting a staged tarball into the hosted store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TarballFinalize {
    /// The tarball was promoted: created on S3, or renamed into place on the
    /// single-node FS backend, which owns its store exclusively.
    Written,
    /// An object with byte-identical content already occupied the key, so
    /// promotion was a no-op. Safe — the published artifact is exactly ours.
    AlreadyIdentical,
    /// A *different* object already occupies the key: a concurrent publisher
    /// won this version's tarball. A published version's tarball is immutable,
    /// so the caller must not overwrite it and should surface a write conflict
    /// rather than advertise an integrity that no longer matches the bytes.
    Conflict,
}

impl HostedStore {
    async fn read_packument(&self, name: &PackageName) -> Result<Option<Vec<u8>>> {
        match self {
            HostedStore::Fs(store) => store.read_packument_any_age(name).await,
            HostedStore::S3(store) => store.read_packument(name).await,
        }
    }

    async fn read_packument_for_update(
        &self,
        name: &PackageName,
    ) -> Result<Option<HostedPackumentForUpdate>> {
        match self {
            HostedStore::Fs(store) => Ok(store.read_packument_any_age(name).await?.map(|bytes| {
                HostedPackumentForUpdate { bytes, version: HostedPackumentVersion::Fs }
            })),
            HostedStore::S3(store) => {
                Ok(store.read_packument_for_update(name).await?.map(|packument| {
                    HostedPackumentForUpdate {
                        bytes: packument.bytes,
                        version: HostedPackumentVersion::S3(packument.version),
                    }
                }))
            }
        }
    }

    /// FS has no hosted packument CAS and always returns `Written`.
    /// `Conflict` is only returned by the S3 object-version path.
    async fn write_packument_if_current(
        &self,
        name: &PackageName,
        bytes: &[u8],
        version: Option<&HostedPackumentVersion>,
    ) -> Result<PackumentWrite> {
        match self {
            HostedStore::Fs(store) => {
                store.write_packument(name, bytes).await?;
                Ok(PackumentWrite::Written)
            }
            HostedStore::S3(store) => {
                let version = match version {
                    Some(HostedPackumentVersion::S3(version)) => Some(version),
                    Some(HostedPackumentVersion::Fs) | None => None,
                };
                if store.write_packument_if_current(name, bytes, version).await? {
                    Ok(PackumentWrite::Written)
                } else {
                    Ok(PackumentWrite::Conflict)
                }
            }
        }
    }

    async fn open_tarball(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<Option<(Body, Option<u64>)>> {
        match self {
            HostedStore::Fs(store) => Ok(store
                .open_tarball(name, filename)
                .await?
                .map(|(file, len)| (streaming::stream_file(file), Some(len)))),
            HostedStore::S3(store) => store.open_tarball(name, filename).await,
        }
    }

    /// Reserve the local staging path the publish flow decodes into.
    async fn reserve_tarball_tmp(&self, name: &PackageName, filename: &str) -> Result<PathBuf> {
        match self {
            HostedStore::Fs(store) => store.reserve_tarball_tmp(name, filename).await,
            HostedStore::S3(store) => store.staging_tmp_path(name, filename).await,
        }
    }

    async fn finalize_tarball(
        &self,
        tmp_path: &Path,
        name: &PackageName,
        filename: &str,
    ) -> Result<TarballFinalize> {
        match self {
            HostedStore::Fs(store) => {
                store.finalize_tarball(tmp_path, name, filename).await?;
                Ok(TarballFinalize::Written)
            }
            HostedStore::S3(store) => {
                let outcome = store.upload_tarball(tmp_path, name, filename).await?;
                // Keep the staged tmp on a Conflict so journal roll-forward can
                // re-detect it and exclude the version whose bytes we don't own;
                // once the object is ours there is nothing left to promote.
                if outcome != TarballFinalize::Conflict {
                    let _ = fs::remove_file(tmp_path).await;
                }
                Ok(outcome)
            }
        }
    }

    async fn remove_tarball(&self, name: &PackageName, filename: &str) -> Result<bool> {
        match self {
            HostedStore::Fs(store) => store.remove_tarball(name, filename).await,
            HostedStore::S3(store) => store.remove_tarball(name, filename).await,
        }
    }

    async fn remove_package(&self, name: &PackageName) -> Result<bool> {
        match self {
            HostedStore::Fs(store) => store.remove_package(name).await,
            HostedStore::S3(store) => store.remove_package(name).await,
        }
    }

    async fn list_package_names(&self) -> Result<Vec<String>> {
        match self {
            HostedStore::Fs(store) => store.list_package_names().await,
            HostedStore::S3(store) => store.list_package_names().await,
        }
    }

    /// A view rooted under `segment`, giving a hosted registry its own
    /// storage namespace so two orgs hosting the same `name@version` never
    /// collide on disk (or on object keys).
    fn namespaced(&self, segment: &str) -> HostedStore {
        match self {
            HostedStore::Fs(store) => HostedStore::Fs(store.namespaced(segment)),
            HostedStore::S3(store) => HostedStore::S3(store.namespaced(segment)),
        }
    }

    async fn read_staged(&self, object: &str) -> Result<Option<Vec<u8>>> {
        match self {
            HostedStore::Fs(store) => store.read_staged(object).await,
            HostedStore::S3(store) => store.read_staged(object).await,
        }
    }

    async fn write_staged(&self, object: &str, bytes: &[u8]) -> Result<()> {
        match self {
            HostedStore::Fs(store) => store.write_staged(object, bytes).await,
            HostedStore::S3(store) => store.write_staged(object, bytes).await,
        }
    }

    async fn remove_staged(&self, object: &str) -> Result<bool> {
        match self {
            HostedStore::Fs(store) => store.remove_staged(object).await,
            HostedStore::S3(store) => store.remove_staged(object).await,
        }
    }

    async fn list_staged_ids(&self) -> Result<Vec<String>> {
        match self {
            HostedStore::Fs(store) => store.list_staged_ids().await,
            HostedStore::S3(store) => store.list_staged_ids().await,
        }
    }
}

impl Storage {
    /// Build a [`Storage`] from the resolved hosted-store backend plus
    /// the local `storage` and `cache_storage` roots. `storage` backs
    /// the hosted store when it's [`HostedStoreConfig::Fs`];
    /// `cache_storage` always backs the proxy cache and doubles as the
    /// S3 backend's local staging scratch.
    pub fn new(hosted: &HostedStoreConfig, storage: PathBuf, cache_storage: PathBuf) -> Self {
        let cached = Store::new(cache_storage.clone());
        let hosted = match hosted {
            HostedStoreConfig::Fs => HostedStore::Fs(Store::new(storage)),
            HostedStoreConfig::S3 { store, prefix } => {
                HostedStore::S3(S3Store::new(Arc::clone(store), prefix.clone(), cache_storage))
            }
        };
        Self { hosted, cached }
    }

    /// The hosted package names, used by the local search scan (which
    /// indexes hosted/static packages only, never the proxy mirror).
    pub async fn hosted_package_names(&self) -> Result<Vec<String>> {
        self.hosted.list_package_names().await
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

    //  Authoritative (hosted) store --------------------------------

    /// Read the authoritative packument for `name`, fresh or stale.
    /// Hosted content has no TTL — it is the source of truth.
    pub async fn read_hosted_packument(&self, name: &PackageName) -> Result<Option<Vec<u8>>> {
        self.hosted.read_packument(name).await
    }

    pub(crate) async fn read_hosted_packument_for_update(
        &self,
        name: &PackageName,
    ) -> Result<Option<HostedPackumentForUpdate>> {
        self.hosted.read_packument_for_update(name).await
    }

    pub(crate) async fn write_hosted_packument_if_current(
        &self,
        name: &PackageName,
        bytes: &[u8],
        version: Option<&HostedPackumentVersion>,
    ) -> Result<PackumentWrite> {
        self.hosted.write_packument_if_current(name, bytes, version).await
    }

    /// Read the hosted packument, transform it, and conditionally write it
    /// back under compare-and-swap, retrying on conflict with capped backoff.
    ///
    /// `build` receives the current hosted bytes (`None` when the packument is
    /// absent) and returns the bytes to write, or `Ok(None)` to abort as
    /// [`PackumentUpdate::NotFound`]; a `build` error aborts without retrying.
    /// After `retries` conflicts the write is surfaced as
    /// [`RegistryError::PackumentWriteConflict`]. Both the dist-tag request
    /// path and journal roll-forward go through here so their conflict handling
    /// stays in one place.
    pub(crate) async fn update_hosted_packument_with_retry<Build>(
        &self,
        name: &PackageName,
        retries: usize,
        mut build: Build,
    ) -> Result<PackumentUpdate>
    where
        Build: FnMut(Option<&[u8]>) -> Result<Option<Vec<u8>>>,
    {
        for attempt in 0..retries {
            let existing = self.read_hosted_packument_for_update(name).await?;
            let (existing_bytes, version) = match existing {
                Some(packument) => (Some(packument.bytes), Some(packument.version)),
                None => (None, None),
            };
            let Some(new_bytes) = build(existing_bytes.as_deref())? else {
                return Ok(PackumentUpdate::NotFound);
            };
            match self.write_hosted_packument_if_current(name, &new_bytes, version.as_ref()).await?
            {
                PackumentWrite::Written => return Ok(PackumentUpdate::Written),
                PackumentWrite::Conflict => {
                    if attempt + 1 < retries {
                        wait_after_packument_write_conflict(attempt).await;
                    }
                }
            }
        }
        Err(RegistryError::PackumentWriteConflict { package: name.as_str().to_string() })
    }

    /// Open a tarball from the authoritative hosted store. Hosted
    /// publish writes verify their SRI before finalization, and static
    /// storage remains operator-controlled rather than an upstream cache.
    pub async fn open_hosted_tarball(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<Option<(Body, Option<u64>)>> {
        self.hosted.open_tarball(name, filename).await
    }

    /// Reserve a staging slot for a tarball this server hosts. The
    /// publish flow streams the decode + hash + write through
    /// `std::fs` inside `spawn_blocking` and only needs the path;
    /// finalize with [`Self::finalize_tarball_slot`].
    pub async fn reserve_hosted_tarball(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<TarballSlot> {
        let tmp_path = self.hosted.reserve_tarball_tmp(name, filename).await?;
        Ok(TarballSlot { tmp_path, name: name.clone(), filename: filename.to_string() })
    }

    /// Remove a single tarball file from both stores. The
    /// partial-unpublish flow calls this after PUT'ing the modified
    /// packument back; clearing the proxied mirror too stops
    /// the proxy cache from serving a stale copy of the just-removed
    /// version.
    pub async fn remove_tarball(&self, name: &PackageName, filename: &str) -> Result<bool> {
        let hosted = self.hosted.remove_tarball(name, filename).await?;
        let cached = self.cached.remove_tarball(name, filename).await?;
        Ok(hosted || cached)
    }

    /// Remove the package from both stores. Unpublish must purge the
    /// hosted copy *and* any proxied mirror, so a stale cached copy
    /// can't resurface after the package is gone.
    pub async fn remove_package(&self, name: &PackageName) -> Result<bool> {
        let hosted = self.hosted.remove_package(name).await?;
        let cached = self.cached.remove_package(name).await?;
        Ok(hosted || cached)
    }

    //  Per-upstream private cache (the `/~<name>/` registry endpoint) -
    //
    // A private upstream's packuments and tarballs are cached under a namespace
    // derived from the upstream and its rotation generation, kept separate from
    // the shared public mirror so they can never be served on the public path
    // or under another upstream. A rotation (new generation) moves to a fresh
    // namespace, so entries fetched with a since-rotated credential age out.

    /// A fresh cached packument for an upstream route, or `None` when it is
    /// absent or older than `ttl`. The upstream path refetches a stale entry
    /// rather than conditionally revalidating it.
    pub async fn read_upstream_packument(
        &self,
        namespace: &str,
        name: &PackageName,
        ttl: Duration,
    ) -> Result<Option<Vec<u8>>> {
        match self.cached.namespaced(namespace).read_packument_entry(name, ttl).await? {
            Some(CachedPackument::Fresh(bytes)) => Ok(Some(bytes)),
            Some(CachedPackument::Stale) | None => Ok(None),
        }
    }

    /// The cached upstream packument regardless of freshness (fresh or stale).
    /// A defensive fallback for an unsolicited upstream `304`: the upstream path
    /// sends no conditional validators, so a `304` means "unchanged" and the
    /// cached body — even past `ttl` — is the right thing to serve rather than
    /// a spurious `404`.
    pub async fn read_upstream_packument_any(
        &self,
        namespace: &str,
        name: &PackageName,
    ) -> Result<Option<Vec<u8>>> {
        // `Duration::MAX` classifies any existing entry as fresh, so its body
        // is returned regardless of age (the stale arm can't be reached here).
        match self.cached.namespaced(namespace).read_packument_entry(name, Duration::MAX).await? {
            Some(CachedPackument::Fresh(bytes)) => Ok(Some(bytes)),
            Some(CachedPackument::Stale) | None => Ok(None),
        }
    }

    pub async fn write_upstream_packument(
        &self,
        namespace: &str,
        name: &PackageName,
        bytes: &[u8],
    ) -> Result<()> {
        self.cached.namespaced(namespace).write_packument(name, bytes).await
    }

    /// Purge an upstream's cached entry for `name` — the packument and any
    /// cached tarballs. Called on a definitive upstream 404: without the
    /// purge, the stale entry would linger past its TTL and a later transient
    /// outage could resurrect the unpublished package through the
    /// stale-if-error fallback.
    pub async fn remove_upstream_package(
        &self,
        namespace: &str,
        name: &PackageName,
    ) -> Result<bool> {
        self.cached.namespaced(namespace).remove_package(name).await
    }

    pub async fn open_upstream_tarball_tmp(
        &self,
        namespace: &str,
        name: &PackageName,
        filename: &str,
    ) -> Result<TarballWrite> {
        self.cached.namespaced(namespace).open_tarball_tmp(name, filename).await
    }

    pub async fn open_upstream_tarball(
        &self,
        namespace: &str,
        name: &PackageName,
        filename: &str,
    ) -> Result<Option<(fs::File, u64)>> {
        self.cached.namespaced(namespace).open_tarball(name, filename).await
    }

    /// Promote a tmp tarball written by the publish flow to its final
    /// home: a rename on the fs backend, an upload on the S3 backend.
    pub async fn finalize_tarball_slot(&self, slot: TarballSlot) -> Result<TarballFinalize> {
        self.hosted.finalize_tarball(&slot.tmp_path, &slot.name, &slot.filename).await
    }

    /// The commit journal for this storage's publish flow. It lives in
    /// the same local root as the staged tmp files: the hosted store
    /// root on the fs backend, the cache scratch on the S3 backend
    /// (whose staging paths live there too).
    pub fn publish_journal(&self) -> crate::journal::PublishJournal {
        let root = match &self.hosted {
            HostedStore::Fs(store) => &store.root,
            HostedStore::S3(_) => &self.cached.root,
        };
        crate::journal::PublishJournal::new(root.join(crate::journal::JOURNAL_DIR))
    }

    //  Staged publishes (`-/stage`) ----------------------------------
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
}

impl Store {
    fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// A disposable store rooted at a sub-path of this one. Used to give a
    /// private `/~<name>/` route its own cache namespace so its packuments
    /// and tarballs never collide with the public mirror or another upstream.
    fn namespaced(&self, prefix: &str) -> Store {
        Store::new(self.root.join(prefix))
    }

    async fn read_packument_entry(
        &self,
        name: &PackageName,
        ttl: Duration,
    ) -> Result<Option<CachedPackument>> {
        let path = self.packument_path(name);
        let metadata = match fs::metadata(&path).await {
            Ok(m) => m,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let mtime = metadata.modified().map_err(RegistryError::Io)?;
        let age = SystemTime::now().duration_since(mtime).unwrap_or(Duration::ZERO);
        if age <= ttl {
            // Fresh: read the body and serve it.
            Ok(Some(CachedPackument::Fresh(fs::read(&path).await?)))
        } else {
            // Stale: treated as a miss so the caller refetches from the upstream
            // (there is no conditional revalidation), so the body isn't read here.
            Ok(Some(CachedPackument::Stale))
        }
    }

    async fn read_packument_any_age(&self, name: &PackageName) -> Result<Option<Vec<u8>>> {
        let path = self.packument_path(name);
        match fs::read(&path).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    async fn write_packument(&self, name: &PackageName, bytes: &[u8]) -> Result<()> {
        let path = self.packument_path(name);
        write_atomic(&path, bytes).await
    }

    async fn open_tarball(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<Option<(fs::File, u64)>> {
        let path = self.tarball_path(name, filename);
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

    async fn open_tarball_tmp(&self, name: &PackageName, filename: &str) -> Result<TarballWrite> {
        let final_path = self.tarball_path(name, filename);
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let (file, tmp_path) = create_tmp_file(&final_path).await?;
        Ok(TarballWrite { file: Some(file), tmp_path: Some(tmp_path), final_path })
    }

    /// Reserve a tmp path in the destination package directory so the
    /// publish flow can write there and [`Self::finalize_tarball`] can
    /// rename within the same directory (atomic on POSIX).
    async fn reserve_tarball_tmp(&self, name: &PackageName, filename: &str) -> Result<PathBuf> {
        let final_path = self.tarball_path(name, filename);
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        Ok(unique_tmp_path(&final_path))
    }

    async fn finalize_tarball(
        &self,
        tmp_path: &Path,
        name: &PackageName,
        filename: &str,
    ) -> Result<()> {
        let final_path = self.tarball_path(name, filename);
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::rename(tmp_path, &final_path).await?;
        Ok(())
    }

    /// Remove the entire package directory. Returns `Ok(false)` if it
    /// didn't exist (treat as a no-op success, matching what verdaccio
    /// does on a duplicate DELETE).
    async fn remove_package(&self, name: &PackageName) -> Result<bool> {
        let dir = self.package_dir(name);
        match fs::remove_dir_all(&dir).await {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    /// Remove a single tarball file. Returns `Ok(false)` when the file
    /// is already gone; the pnpm unpublish flow always issues a DELETE
    /// after the packument-update PUT, and a benign 404 here would
    /// surface as a real error to the caller.
    async fn remove_tarball(&self, name: &PackageName, filename: &str) -> Result<bool> {
        match fs::remove_file(self.tarball_path(name, filename)).await {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    /// Walk the storage tree two levels deep to find package names —
    /// directories holding a `package.json`. Layout is
    /// `<root>/<pkg>/package.json` for unscoped and
    /// `<root>/@scope/<name>/package.json` for scoped, so a two-level
    /// walk suffices and avoids descending into tarball-adjacent junk.
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
            if fs::try_exists(entry_path.join(PACKUMENT_FILE)).await.unwrap_or(false) {
                names.push(name_str.into_owned());
                continue;
            }
            if name_str.starts_with('@')
                && let Ok(mut inner) = fs::read_dir(&entry_path).await
            {
                while let Some(child) = inner.next_entry().await? {
                    if fs::try_exists(child.path().join(PACKUMENT_FILE)).await.unwrap_or(false) {
                        names.push(format!("{name_str}/{}", child.file_name().to_string_lossy()));
                    }
                }
            }
        }
        Ok(names)
    }

    fn package_dir(&self, name: &PackageName) -> PathBuf {
        self.root.join(name.as_str())
    }

    fn packument_path(&self, name: &PackageName) -> PathBuf {
        self.package_dir(name).join(PACKUMENT_FILE)
    }

    fn tarball_path(&self, name: &PackageName, filename: &str) -> PathBuf {
        self.package_dir(name).join(filename)
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

/// The stage id of a metadata object name, or `None` for anything else in
/// the staged namespace (bodies, tmp files from interrupted writes).
pub(crate) fn staged_id_of_meta_object(object: &str) -> Option<&str> {
    if object.ends_with(STAGED_BODY_SUFFIX) {
        return None;
    }
    object.strip_suffix(STAGED_META_SUFFIX)
}

async fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
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
/// POSIX. Shared with [`crate::s3`]'s staging path.
pub(crate) fn unique_tmp_path(base: &Path) -> PathBuf {
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
