use crate::{
    error::{RegistryError, Result},
    package_name::PackageName,
};
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};
use tokio::{fs, io::AsyncWriteExt};

const PACKUMENT_FILE: &str = "package.json";

/// Per-process counter feeding [`unique_tmp_path`] so two concurrent
/// writes to the same path don't collide on the same temp filename.
/// Combined with the pid the suffix is unique across every writer this
/// process spawns; the rename is still atomic on POSIX as long as src
/// and dest sit in the same directory (they do).
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Handle returned from [`Cache::open_cached_tarball_tmp`]. The caller
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

/// A reserved (tmp_path, final_path) pair for a tarball write. The
/// publish flow writes the tarball to `tmp_path` inside a blocking
/// task, then renames via [`Cache::finalize_tarball_slot`].
#[derive(Debug)]
pub struct TarballSlot {
    pub tmp_path: PathBuf,
    pub final_path: PathBuf,
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

/// Verdaccio-shaped on-disk storage split into two physically separate
/// roots with different durability guarantees:
///
/// * `hosted` — the authoritative source of truth: packages this
///   server hosts directly (published through its API) plus the content
///   served in static mode. Served as-is and never overwritten by an
///   upstream refresh, so a hosted version can't be masked or lost.
///   Operators back this up and put it on a durable volume.
/// * `cache` — the disposable mirror of upstream registries. Safe to
///   wipe at any time; it self-heals on the next request. Operators can
///   keep it on scratch/ephemeral disk.
///
/// Both roots use the same on-disk layout:
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
pub struct Cache {
    hosted: Store,
    cache: Store,
}

impl Cache {
    pub fn new(hosted_root: PathBuf, cache_root: PathBuf) -> Self {
        Self { hosted: Store::new(hosted_root), cache: Store::new(cache_root) }
    }

    /// The hosted store's root, used by the local search scan (which
    /// indexes hosted/static packages only, never the proxy mirror).
    pub fn hosted_root(&self) -> &Path {
        &self.hosted.root
    }

    // --- Authoritative (hosted) store -----------------------------------

    /// Read the authoritative packument for `name`, fresh or stale.
    /// Hosted content has no TTL — it is the source of truth.
    pub async fn read_hosted_packument(&self, name: &PackageName) -> Result<Option<Vec<u8>>> {
        self.hosted.read_packument_any_age(name).await
    }

    pub async fn write_hosted_packument(&self, name: &PackageName, bytes: &[u8]) -> Result<()> {
        self.hosted.write_packument(name, bytes).await
    }

    /// Reserve a tmp/final path pair for a tarball this server hosts.
    /// The publish flow streams the decode + hash + write through
    /// `std::fs` inside `spawn_blocking` and only needs the paths;
    /// finalize with [`Self::finalize_tarball_slot`].
    pub async fn reserve_hosted_tarball(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<TarballSlot> {
        self.hosted.reserve_tarball_paths(name, filename).await
    }

    /// Remove a single tarball file from both stores. The
    /// partial-unpublish flow calls this after PUT'ing the modified
    /// packument back; clearing the proxied mirror too stops
    /// [`Self::open_tarball`]'s cache fallback from serving a stale copy
    /// of the just-removed version.
    pub async fn remove_tarball(&self, name: &PackageName, filename: &str) -> Result<bool> {
        let hosted = self.hosted.remove_tarball(name, filename).await?;
        let cached = self.cache.remove_tarball(name, filename).await?;
        Ok(hosted || cached)
    }

    /// Remove the package from both stores. Unpublish must purge the
    /// hosted copy *and* any proxied mirror, so a stale cached copy
    /// can't resurface after the package is gone.
    pub async fn remove_package(&self, name: &PackageName) -> Result<bool> {
        let hosted = self.hosted.remove_package(name).await?;
        let cached = self.cache.remove_package(name).await?;
        Ok(hosted || cached)
    }

    // --- Disposable (proxy) cache store ---------------------------------

    /// Read a cached upstream packument if it exists and is newer than
    /// `now - ttl`.
    pub async fn read_fresh_cached_packument(
        &self,
        name: &PackageName,
        ttl: Duration,
    ) -> Result<Option<Vec<u8>>> {
        self.cache.read_fresh_packument(name, ttl).await
    }

    /// Read whatever cached upstream packument is on disk, fresh or
    /// stale. Used as a fallback when the upstream is unreachable.
    pub async fn read_cached_packument(&self, name: &PackageName) -> Result<Option<Vec<u8>>> {
        self.cache.read_packument_any_age(name).await
    }

    pub async fn write_cached_packument(&self, name: &PackageName, bytes: &[u8]) -> Result<()> {
        self.cache.write_packument(name, bytes).await
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
        self.cache.open_tarball_tmp(name, filename).await
    }

    // --- Composed (hosted-first) ----------------------------------------

    /// Open a tarball for streaming, preferring the hosted store over
    /// the proxy mirror. Returns the open file plus its size (for
    /// `Content-Length`). `Ok(None)` means neither store has it, so the
    /// caller can fall through to the upstream fetch.
    pub async fn open_tarball(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<Option<(fs::File, u64)>> {
        if let Some(hit) = self.hosted.open_tarball(name, filename).await? {
            return Ok(Some(hit));
        }
        self.cache.open_tarball(name, filename).await
    }

    /// Atomically promote a tmp tarball written by the publish flow to
    /// its final path. Mirrors what [`TarballWrite::finalize`] does,
    /// minus the `sync_all` (the blocking task that wrote the file
    /// already synced it). Store-agnostic: the slot already carries its
    /// final path.
    pub async fn finalize_tarball_slot(&self, slot: TarballSlot) -> Result<()> {
        if let Some(parent) = slot.final_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::rename(&slot.tmp_path, &slot.final_path).await?;
        Ok(())
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

    async fn read_fresh_packument(
        &self,
        name: &PackageName,
        ttl: Duration,
    ) -> Result<Option<Vec<u8>>> {
        let path = self.packument_path(name);
        let metadata = match fs::metadata(&path).await {
            Ok(m) => m,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        let mtime = metadata.modified().map_err(RegistryError::Io)?;
        let age = SystemTime::now().duration_since(mtime).unwrap_or(Duration::ZERO);
        if age > ttl {
            return Ok(None);
        }
        Ok(Some(fs::read(&path).await?))
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

    async fn reserve_tarball_paths(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<TarballSlot> {
        let final_path = self.tarball_path(name, filename);
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let tmp_path = unique_tmp_path(&final_path);
        Ok(TarballSlot { tmp_path, final_path })
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

    fn package_dir(&self, name: &PackageName) -> PathBuf {
        self.root.join(name.as_str())
    }

    fn packument_path(&self, name: &PackageName) -> PathBuf {
        self.package_dir(name).join(PACKUMENT_FILE)
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

fn unique_tmp_path(base: &Path) -> PathBuf {
    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let mut name = base.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(format!(".tmp.{pid}.{counter}"));
    match base.parent() {
        Some(parent) => parent.join(name),
        None => PathBuf::from(name),
    }
}
