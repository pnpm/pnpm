use super::{
    AsyncSeekExt, AsyncWriteExt, CanonicalPackageName, ErrorKind, Path, PathBuf, SeekFrom, fs,
};
use std::{
    collections::HashSet,
    sync::{LazyLock, Mutex, PoisonError},
};

static ACTIVE_TEMPS: LazyLock<Mutex<HashSet<PathBuf>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Registers an in-flight temporary file path against [`crate::sweep_stale_temps`]
/// and removes the file if dropped before [`Self::disarm`].
#[derive(Debug)]
pub struct TempFileGuard(Option<PathBuf>);

impl TempFileGuard {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        ACTIVE_TEMPS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.clone());
        Self(Some(path))
    }

    pub fn disarm(&mut self) {
        if let Some(path) = self.0.take() {
            ACTIVE_TEMPS
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&path);
        }
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let Some(path) = self.0.take() else { return };
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => {
                tracing::warn!(
                    ?err,
                    path = %path.display(),
                    "archive staging temp cleanup failed",
                );
            }
        }
        ACTIVE_TEMPS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&path);
    }
}

/// Whether `path` is registered by an active [`TempFileGuard`] in this process.
#[must_use]
pub fn is_active_temp(path: &Path) -> bool {
    ACTIVE_TEMPS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .contains(path)
}

/// Handle returned from [`crate::Storage::open_upstream_blob_tmp`]. The caller
/// writes through [`Self::write_all`] (and on success calls [`Self::finalize`] to
/// atomically promote the temp file to the final cache path). The temp
/// path remains armed until promotion succeeds, so cancellation and
/// every error path remove it through [`Drop`].
pub struct BlobWrite {
    pub(super) file: Option<fs::File>,
    pub(super) tmp_path: Option<PathBuf>,
    pub(super) final_path: PathBuf,
}

/// A reserved slot for a hosted-archive write. The publish flow writes
/// the decoded + verified bytes to `tmp_path` (a local file) inside a
/// blocking task, then promotes it under the identity of those bytes — a
/// rename on the fs backend, an upload on the S3 backend — via
/// [`crate::Storage::promote_blob_slot`]. Dropping an unpromoted slot
/// removes `tmp_path`.
#[derive(Debug)]
pub struct BlobSlot {
    pub tmp_path: PathBuf,
    pub(super) name: CanonicalPackageName,
    /// The public file name the archive is published as.
    pub(super) filename: String,
    /// The version the file belongs to, empty when none.
    pub(super) version: String,
    pub(super) guard: TempFileGuard,
}

impl BlobSlot {
    /// The public file name the archive is published as.
    #[must_use]
    pub fn filename(&self) -> &str {
        &self.filename
    }

    #[must_use]
    pub fn name(&self) -> &CanonicalPackageName {
        &self.name
    }

    /// The slot of a file of `version`, whose mapping goes with the version.
    #[must_use]
    pub fn for_version(mut self, version: &str) -> Self {
        self.version = version.to_string();
        self
    }

    /// Write `bytes` to `tmp_path` on a blocking thread while keeping slot
    /// ownership inside the blocking task, so cancelling the caller's future
    /// removes `tmp_path` after the write finishes rather than before it
    /// creates the file.
    pub async fn write_bytes(self, bytes: &[u8]) -> super::Result<Self> {
        let bytes = bytes.to_vec();
        self.run_blocking_write(move |path| std::fs::write(path, bytes)).await
    }

    pub(crate) async fn run_blocking_write(
        self,
        write: impl FnOnce(&super::Path) -> std::io::Result<()> + Send + 'static,
    ) -> super::Result<Self> {
        tokio::task::spawn_blocking(move || {
            write(&self.tmp_path)?;
            Ok(self)
        })
        .await
        .map_err(|err| pnpr_error::RegistryError::Io(std::io::Error::other(err.to_string())))?
    }

    pub async fn abandon(mut self) {
        match fs::remove_file(&self.tmp_path).await {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => {
                tracing::warn!(
                    ?err,
                    path = %self.tmp_path.display(),
                    "archive staging temp cleanup failed",
                );
            }
        }
        self.guard.disarm();
    }
}

impl BlobWrite {
    pub async fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.write_all(bytes).await,
            None => Err(std::io::Error::other("blob cache writer is closed")),
        }
    }

    /// Read back what was written through `check` on a blocking thread and
    /// return its verdict. The handle stays open, so the bytes checked are the
    /// bytes later promoted or streamed.
    pub async fn check(
        &mut self,
        check: impl FnOnce(&mut std::fs::File) -> bool + Send + 'static,
    ) -> std::io::Result<bool> {
        let Some(mut file) = self.file.take() else {
            return Err(std::io::Error::other("blob cache writer is closed"));
        };
        file.flush().await?;
        file.seek(SeekFrom::Start(0)).await?;
        let mut std_file = file.into_std().await;
        let (std_file, verdict) = tokio::task::spawn_blocking(move || {
            let verdict = check(&mut std_file);
            (std_file, verdict)
        })
        .await
        .map_err(std::io::Error::other)?;
        let mut file = fs::File::from_std(std_file);
        file.seek(SeekFrom::End(0)).await?;
        self.file = Some(file);
        Ok(verdict)
    }

    /// Sync the file to disk and rename it to its final cache path.
    pub async fn finalize(self) -> std::io::Result<()> {
        self.seal().await?.promote().await
    }

    /// The cache path [`Self::finalize`] promotes this write to.
    #[must_use]
    pub fn final_path(&self) -> &super::Path {
        &self.final_path
    }

    /// Sync the file to disk and close it, leaving it unpromoted.
    pub async fn seal(mut self) -> std::io::Result<SealedBlob> {
        match self.file.take() {
            Some(file) => file.sync_all().await?,
            None => return Err(std::io::Error::other("blob cache writer is closed")),
        }
        Ok(SealedBlob { write: self })
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
        let tmp_path = self.tmp_path
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

/// A [`BlobWrite`] whose bytes are on disk and whose file is closed, so it
/// holds no descriptor while it waits. [`Self::promote`] renames it to its
/// final cache path; dropping it removes the temp file.
pub struct SealedBlob {
    write: BlobWrite,
}

impl SealedBlob {
    pub async fn promote(mut self) -> std::io::Result<()> {
        let write = &mut self.write;
        if let Some(parent) = write.final_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let tmp_path = write.tmp_path
            .as_ref()
            .ok_or_else(|| std::io::Error::other("blob cache temp path is missing"))?;
        fs::rename(tmp_path, &write.final_path).await?;
        write.tmp_path = None;
        Ok(())
    }

    pub async fn abandon(self) {
        self.write.abandon().await;
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
