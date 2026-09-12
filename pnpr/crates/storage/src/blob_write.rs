use super::{AsyncSeekExt, AsyncWriteExt, CanonicalPackageName, ErrorKind, PathBuf, SeekFrom, fs};

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

/// A reserved slot for a hosted-blob write. The publish flow writes
/// the decoded + verified bytes to `tmp_path` (a local file) inside a
/// blocking task, then promotes it to its final home — a rename on the
/// fs backend, an upload on the S3 backend — via
/// [`crate::Storage::finalize_blob_slot`], which recomputes the
/// destination from `name`/`filename`.
#[derive(Debug)]
pub struct BlobSlot {
    pub tmp_path: PathBuf,
    pub(super) name: CanonicalPackageName,
    pub(super) filename: String,
}

impl BlobSlot {
    /// Rebuild a slot from its journaled parts so startup recovery can
    /// re-run [`crate::Storage::finalize_blob_slot`] on it.
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
