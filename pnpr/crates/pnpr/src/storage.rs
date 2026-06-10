use crate::{
    config::HostedStoreConfig,
    error::{RegistryError, Result},
    package_name::PackageName,
    s3::S3Store,
    streaming,
    upstream::CacheValidators,
};
use axum::body::Body;
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};
use tokio::{fs, io::AsyncWriteExt};

const PACKUMENT_FILE: &str = "package.json";

/// Sidecar holding the cached packument's conditional-GET validators
/// (see [`CacheValidators`]). Lives next to [`PACKUMENT_FILE`] in the
/// disposable cache store only; hosted packuments have no upstream to
/// revalidate against. The leading dot keeps it out of the
/// package-listing walk and any static-serve view.
const PACKUMENT_META_FILE: &str = ".package.json.meta";

/// Per-process counter feeding [`unique_tmp_path`] so two concurrent
/// writes to the same path don't collide on the same temp filename.
/// Combined with the pid the suffix is unique across every writer this
/// process spawns; the rename is still atomic on POSIX as long as src
/// and dest sit in the same directory (they do).
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Handle returned from [`Storage::open_cached_tarball_tmp`]. The caller
/// writes to `file` (and on success calls [`Self::finalize`] to
/// atomically promote the temp file to the final cache path); dropping
/// the handle without calling [`Self::finalize`] is treated as abandon
/// — callers that hit an error mid-write should call [`Self::abandon`]
/// to actively remove the leftover temp file.
pub struct TarballWrite {
    pub file: fs::File,
    tmp_path: PathBuf,
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

impl TarballWrite {
    /// Sync the file to disk and rename it to its final cache path.
    pub async fn finalize(self) -> Result<()> {
        self.file.sync_all().await?;
        drop(self.file);
        if let Some(parent) = self.final_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::rename(&self.tmp_path, &self.final_path).await?;
        Ok(())
    }

    /// Remove the temp file. Errors are swallowed since the caller
    /// is already handling a higher-level failure and a leftover
    /// `*.tmp.*` file is harmless beyond a small amount of disk.
    pub async fn abandon(self) {
        drop(self.file);
        let _ = fs::remove_file(&self.tmp_path).await;
    }
}

/// A cached upstream packument, read at a granularity that avoids loading
/// the (potentially multi-MB) body when it isn't needed:
///
/// * `Fresh` — within the TTL; the body is read and ready to serve.
/// * `Stale` — past the TTL; only the small conditional-GET validators
///   are loaded. The body is left on disk and pulled on demand by the
///   caller (via [`Storage::read_cached_packument`]) only if the upstream
///   answers `304` or is unreachable — the common stale→`200` refresh
///   discards the old body, so it's never read.
#[derive(Debug)]
pub enum CachedPackument {
    Fresh(Vec<u8>),
    Stale(CacheValidators),
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

impl HostedStore {
    async fn read_packument(&self, name: &PackageName) -> Result<Option<Vec<u8>>> {
        match self {
            HostedStore::Fs(store) => store.read_packument_any_age(name).await,
            HostedStore::S3(store) => store.read_packument(name).await,
        }
    }

    async fn write_packument(&self, name: &PackageName, bytes: &[u8]) -> Result<()> {
        match self {
            HostedStore::Fs(store) => store.write_packument(name, bytes).await,
            HostedStore::S3(store) => store.write_packument(name, bytes).await,
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
    ) -> Result<()> {
        match self {
            HostedStore::Fs(store) => store.finalize_tarball(tmp_path, name, filename).await,
            HostedStore::S3(store) => {
                store.upload_tarball(tmp_path, name, filename).await?;
                let _ = fs::remove_file(tmp_path).await;
                Ok(())
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

    // --- Authoritative (hosted) store -----------------------------------

    /// Read the authoritative packument for `name`, fresh or stale.
    /// Hosted content has no TTL — it is the source of truth.
    pub async fn read_hosted_packument(&self, name: &PackageName) -> Result<Option<Vec<u8>>> {
        self.hosted.read_packument(name).await
    }

    pub async fn write_hosted_packument(&self, name: &PackageName, bytes: &[u8]) -> Result<()> {
        self.hosted.write_packument(name, bytes).await
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
    /// [`Self::open_tarball`]'s cache fallback from serving a stale copy
    /// of the just-removed version.
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

    // --- Disposable (proxy) cache store ---------------------------------

    /// Classify the cached upstream packument against `ttl`: a
    /// [`CachedPackument::Fresh`] entry comes back with its body ready to
    /// serve; a [`CachedPackument::Stale`] one comes back with only its
    /// validators, deferring the (possibly large) body read to the caller
    /// via [`Self::read_cached_packument`]. Returns `Ok(None)` when
    /// nothing is cached.
    pub async fn read_cached_packument_entry(
        &self,
        name: &PackageName,
        ttl: Duration,
    ) -> Result<Option<CachedPackument>> {
        self.cached.read_packument_entry(name, ttl).await
    }

    /// Read whatever cached upstream packument is on disk, fresh or
    /// stale. Used when there's no upstream left to revalidate against.
    pub async fn read_cached_packument(&self, name: &PackageName) -> Result<Option<Vec<u8>>> {
        self.cached.read_packument_any_age(name).await
    }

    /// Write a cached upstream packument and its validators, refreshing
    /// the entry's freshness. Called both on a fresh upstream body and
    /// on a `304` revalidation (re-written with the unchanged bytes to
    /// bump the cache mtime).
    pub async fn write_cached_packument(
        &self,
        name: &PackageName,
        bytes: &[u8],
        validators: &CacheValidators,
    ) -> Result<()> {
        self.cached.write_packument_with_meta(name, bytes, validators).await
    }

    /// Create and open a per-request temp file for a proxied tarball.
    /// The caller streams bytes into [`TarballWrite::file`] and calls
    /// [`TarballWrite::finalize`] (or [`TarballWrite::abandon`]) when
    /// the upstream response ends.
    pub async fn open_cached_tarball_tmp(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<TarballWrite> {
        self.cached.open_tarball_tmp(name, filename).await
    }

    // --- Composed (hosted-first) ----------------------------------------

    /// Open a tarball for streaming, preferring the hosted store over
    /// the proxy mirror. Returns a response body plus its size (for
    /// `Content-Length`). `Ok(None)` means neither store has it, so the
    /// caller can fall through to the upstream fetch.
    pub async fn open_tarball(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<Option<(Body, Option<u64>)>> {
        if let Some(hit) = self.hosted.open_tarball(name, filename).await? {
            return Ok(Some(hit));
        }
        Ok(self
            .cached
            .open_tarball(name, filename)
            .await?
            .map(|(file, len)| (streaming::stream_file(file), Some(len))))
    }

    /// Promote a tmp tarball written by the publish flow to its final
    /// home: a rename on the fs backend, an upload on the S3 backend.
    pub async fn finalize_tarball_slot(&self, slot: TarballSlot) -> Result<()> {
        self.hosted.finalize_tarball(&slot.tmp_path, &slot.name, &slot.filename).await
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
            // Fresh: read the body to serve it; validators aren't needed.
            Ok(Some(CachedPackument::Fresh(fs::read(&path).await?)))
        } else {
            // Stale: load only the validators for the conditional refetch.
            // The body is read later, on demand, and only if needed.
            Ok(Some(CachedPackument::Stale(self.read_validators(name).await)))
        }
    }

    /// Best-effort read of the validator sidecar. A missing, unreadable,
    /// or malformed sidecar yields empty validators so the next refresh
    /// falls back to an unconditional GET rather than failing.
    async fn read_validators(&self, name: &PackageName) -> CacheValidators {
        match fs::read(self.packument_meta_path(name)).await {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => CacheValidators::default(),
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

    /// Write the packument and persist (or clear) its validator sidecar.
    /// The packument is written first so a sidecar never points at bytes
    /// that aren't on disk yet. When `validators` is empty the sidecar is
    /// removed, so a later read can't replay a stale `ETag` against fresh
    /// bytes that no longer carry one.
    async fn write_packument_with_meta(
        &self,
        name: &PackageName,
        bytes: &[u8],
        validators: &CacheValidators,
    ) -> Result<()> {
        write_atomic(&self.packument_path(name), bytes).await?;
        let meta_path = self.packument_meta_path(name);
        if validators.is_empty() {
            match fs::remove_file(&meta_path).await {
                Ok(()) => {}
                Err(err) if err.kind() == ErrorKind::NotFound => {}
                Err(err) => return Err(err.into()),
            }
        } else {
            write_atomic(&meta_path, &serde_json::to_vec(validators)?).await?;
        }
        Ok(())
    }

    async fn open_tarball(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<Option<(fs::File, u64)>> {
        let path = self.tarball_path(name, filename);
        let file = match fs::File::open(&path).await {
            Ok(f) => f,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
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
        let tmp_path = unique_tmp_path(&final_path);
        let file = fs::File::create(&tmp_path).await?;
        Ok(TarballWrite { file, tmp_path, final_path })
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
        let path = self.tarball_path(name, filename);
        match fs::remove_file(&path).await {
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

    fn packument_meta_path(&self, name: &PackageName) -> PathBuf {
        self.package_dir(name).join(PACKUMENT_META_FILE)
    }

    fn tarball_path(&self, name: &PackageName, filename: &str) -> PathBuf {
        self.package_dir(name).join(filename)
    }
}

async fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let tmp = unique_tmp_path(path);
    let mut file = fs::File::create(&tmp).await?;
    file.write_all(bytes).await?;
    file.sync_all().await?;
    drop(file);
    fs::rename(&tmp, path).await?;
    Ok(())
}

/// A unique sibling of `base` (`<base>.tmp.<pid>.<counter>`). The pid +
/// per-process counter make the suffix unique across every writer this
/// process spawns; keeping it in `base`'s directory keeps the eventual
/// rename atomic on POSIX. Shared with [`crate::s3`]'s staging path.
pub(crate) fn unique_tmp_path(base: &Path) -> PathBuf {
    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let mut name = base.file_name().map(std::ffi::OsStr::to_os_string).unwrap_or_default();
    name.push(format!(".tmp.{pid}.{counter}"));
    match base.parent() {
        Some(parent) => parent.join(name),
        None => PathBuf::from(name),
    }
}

#[cfg(test)]
mod tests;
