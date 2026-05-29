use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::error::{RegistryError, Result};
use crate::package_name::PackageName;

const PACKUMENT_FILE: &str = "package.json";

/// Per-process counter feeding [`unique_tmp_path`] so two concurrent
/// writes to the same path don't collide on the same temp filename.
/// Combined with the pid the suffix is unique across every writer this
/// process spawns; the rename is still atomic on POSIX as long as src
/// and dest sit in the same directory (they do).
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Verdaccio-shaped on-disk storage for packuments and tarballs:
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
    root: PathBuf,
}

/// Handle returned from [`Cache::open_tarball_tmp`]. The caller writes
/// to `file` (and on success calls [`Self::finalize`] to atomically
/// promote the temp file to the final cache path); dropping the
/// handle without calling [`Self::finalize`] is treated as abandon —
/// callers that hit an error mid-write should call [`Self::abandon`]
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

impl Cache {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Read a cached packument if it exists and is newer than
    /// `now - ttl`.
    pub async fn read_fresh_packument(
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

    /// Read whatever packument is on disk, fresh or stale. Used in
    /// static mode (TTL doesn't apply) and as a fallback when the
    /// upstream is unreachable.
    pub async fn read_packument_any_age(&self, name: &PackageName) -> Result<Option<Vec<u8>>> {
        let path = self.packument_path(name);
        match fs::read(&path).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn write_packument(&self, name: &PackageName, bytes: &[u8]) -> Result<()> {
        let path = self.packument_path(name);
        write_atomic(&path, bytes).await
    }

    /// Open the cached tarball file for streaming, if it exists.
    /// Returns the open file plus its size (for `Content-Length`).
    pub async fn open_tarball(
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

    /// Create and open a per-request temp file for a tarball. The
    /// caller streams bytes into [`TarballWrite::file`] and calls
    /// [`TarballWrite::finalize`] (or [`TarballWrite::abandon`]) when
    /// the upstream response ends.
    pub async fn open_tarball_tmp(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<TarballWrite> {
        let final_path = self.tarball_path(name, filename);
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let tmp_path = unique_tmp_path(&final_path);
        let file = fs::File::create(&tmp_path).await?;
        Ok(TarballWrite { file, tmp_path, final_path })
    }

    /// Reserve a tmp/final path pair for a tarball write without
    /// opening the file. Used by the publish flow, which streams the
    /// decode + hash + write through `std::fs` inside
    /// `spawn_blocking` and only needs the paths.
    pub async fn reserve_tarball_paths(
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

    /// Atomically promote a tmp tarball written by the publish flow to
    /// its final cache path. Mirrors what [`TarballWrite::finalize`]
    /// does, minus the `sync_all` (the blocking task that wrote the
    /// file already synced it).
    pub async fn finalize_tarball_slot(&self, slot: TarballSlot) -> Result<()> {
        if let Some(parent) = slot.final_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::rename(&slot.tmp_path, &slot.final_path).await?;
        Ok(())
    }

    /// Remove the entire package directory. Returns `Ok(false)` if it
    /// didn't exist (treat as a no-op success, matching what verdaccio
    /// does on a duplicate DELETE).
    pub async fn remove_package(&self, name: &PackageName) -> Result<bool> {
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
    pub async fn remove_tarball(&self, name: &PackageName, filename: &str) -> Result<bool> {
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
