//! Resumable blob uploads.
//!
//! Filesystem uploads append to a local file. Object-store uploads commit
//! immutable chunks through a conditional session record, so any replica can
//! resume at the accepted offset. Completion materializes the bytes locally
//! for digest verification before promotion to a hosted blob.

pub(crate) use remote::RemoteUploadStore;

mod remote;

use crate::{BlobFinalize, BlobSlot, Storage};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::CanonicalPackageName;
use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    fs,
    io::{AsyncWriteExt, ErrorKind},
};

/// Where in-progress uploads live under the backend's local scratch root.
/// The dot prefix keeps it out of the hosted store's package walk.
pub(crate) const UPLOADS_DIR: &str = ".pnpr-uploads";

/// Where an upload records the repository it was started for. The id alone
/// would let a caller who learns one finish it into a different repository,
/// with bytes they never sent.
const REPOSITORY_SUFFIX: &str = ".repository";

/// How long an untouched upload is kept before it is reclaimed.
///
/// A client that disconnects mid-push leaves its bytes behind, and nothing
/// else would ever remove them: the id is the only handle on an upload, and
/// only the client that started it holds one. A day is far longer than any
/// push and short enough that abandoned ones do not accumulate.
pub const UPLOAD_MAX_AGE: Duration = Duration::from_hours(24);

/// One in-progress blob upload.
#[derive(Debug, Clone)]
pub struct BlobUpload {
    id: String,
    path: PathBuf,
    remote: Option<Arc<remote::RemoteUpload>>,
    _temp: Option<Arc<tempfile::TempPath>>,
}

impl BlobUpload {
    /// The id the client quotes back on every later request of this upload.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// How many bytes have been accepted so far, which is where the next
    /// chunk must start.
    pub async fn offset(&self) -> Result<u64> {
        if let Some(remote) = &self.remote {
            return Ok(remote.offset().await);
        }
        match fs::metadata(&self.path).await {
            Ok(metadata) => Ok(metadata.len()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(0),
            Err(error) => Err(RegistryError::Io(error)),
        }
    }

    /// Local path for digest verification. Call [`Self::materialize`] before
    /// reading an object-store upload.
    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Materialize accepted chunks for digest verification and promotion.
    pub async fn materialize(&self) -> Result<()> {
        if let Some(remote) = &self.remote {
            remote.materialize(&self.path).await?;
        }
        Ok(())
    }

    /// The filename of this upload's repository record.
    fn repository_record(&self) -> String {
        repository_record(&self.id)
    }

    /// Open the upload for appending. Held for one request, so a chunk is
    /// written with one open rather than one per buffer.
    pub async fn append(&self) -> Result<BlobUploadWriter> {
        if let Some(remote) = &self.remote {
            return remote.append().await;
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await
            .map_err(RegistryError::Io)?;
        Ok(BlobUploadWriter { file, remote: None })
    }
}

/// The append handle for one request's worth of an upload.
pub struct BlobUploadWriter {
    file: fs::File,
    remote: Option<remote::RemoteChunk>,
}

impl BlobUploadWriter {
    pub async fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        self.file.write_all(bytes).await.map_err(RegistryError::Io)
    }

    /// Flush to disk and report the upload's new length.
    pub async fn finish(self) -> Result<u64> {
        self.file.sync_all().await.map_err(RegistryError::Io)?;
        let size = self.file.metadata().await.map_err(RegistryError::Io)?.len();
        drop(self.file);
        if let Some(chunk) = self.remote {
            return chunk.commit(size).await;
        }
        Ok(size)
    }
}

impl Storage {
    /// Start an upload for `repository` and give it an unguessable id.
    pub async fn begin_blob_upload(&self, repository: &CanonicalPackageName) -> Result<BlobUpload> {
        if let Some(remote) = self.hosted.upload_store() {
            return remote.begin(repository).await;
        }
        let root = self.uploads_root();
        fs::create_dir_all(&root).await.map_err(RegistryError::Io)?;
        let id = generate_upload_id();
        let path = root.join(&id);
        // `create_new` so a colliding id is an error rather than a silent
        // adoption of someone else's upload.
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .await
            .map_err(RegistryError::Io)?;
        fs::write(root.join(repository_record(&id)), self.upload_owner(repository))
            .await
            .map_err(RegistryError::Io)?;
        Ok(BlobUpload { id, path, remote: None, _temp: None })
    }

    /// Reopen an upload of `repository` by id.
    ///
    /// `None` when no such upload is held, and equally when one is held for
    /// another repository or another organization: an id is a capability over
    /// the bytes its own client sent, not over the scratch space they passed
    /// through.
    pub async fn open_blob_upload(
        &self,
        repository: &CanonicalPackageName,
        id: &str,
    ) -> Result<Option<BlobUpload>> {
        if !is_upload_id(id) {
            return Ok(None);
        }
        if let Some(remote) = self.hosted.upload_store() {
            return remote.open(repository, id).await;
        }
        let root = self.uploads_root();
        let held = match fs::read_to_string(root.join(repository_record(id))).await {
            Ok(held) => held,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(RegistryError::Io(error)),
        };
        if held != self.upload_owner(repository) {
            return Ok(None);
        }
        let path = root.join(id);
        match fs::metadata(&path).await {
            Ok(_) => Ok(Some(BlobUpload { id: id.to_string(), path, remote: None, _temp: None })),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(RegistryError::Io(error)),
        }
    }

    /// Discard an upload, reporting whether one was held.
    pub async fn abort_blob_upload(&self, id: &str) -> Result<bool> {
        if !is_upload_id(id) {
            return Ok(false);
        }
        if let Some(remote) = self.hosted.upload_store() {
            return remote.abort(id).await;
        }
        let root = self.uploads_root();
        let _ = fs::remove_file(root.join(repository_record(id))).await;
        match fs::remove_file(root.join(id)).await {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(error) => Err(RegistryError::Io(error)),
        }
    }

    /// Promote verified bytes before closing their shared upload session.
    /// A failed promotion leaves accepted remote chunks available for retry.
    pub async fn finalize_uploaded_blob(
        &self,
        upload: BlobUpload,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<BlobFinalize> {
        let remote = upload.remote.clone();
        let slot = self.stage_uploaded_blob(upload, name, filename).await?;
        let _temp = tempfile::TempPath::try_from_path(slot.tmp_path.clone())?;
        if let Some(remote) = &remote {
            remote.prepare_completion(filename).await?;
        }
        let outcome = self.finalize_blob_slot(slot).await?;
        if outcome != BlobFinalize::Conflict
            && let Some(remote) = remote
            && let Err(error) = remote.close().await
        {
            tracing::warn!(error = %error.log_message(), "promoted upload awaits session cleanup");
        }
        Ok(outcome)
    }

    /// Move a finished upload into a hosted blob slot, ready for
    /// [`Storage::finalize_blob_slot`].
    ///
    /// Shared chunks remain available until their bytes have been promoted.
    async fn stage_uploaded_blob(
        &self,
        upload: BlobUpload,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<BlobSlot> {
        upload.materialize().await?;
        let slot = self.reserve_hosted_blob(name, filename).await?;
        if let Some(parent) = slot.tmp_path.parent() {
            fs::create_dir_all(parent).await.map_err(RegistryError::Io)?;
        }
        // Both paths are under the backend's local scratch root, so this is a
        // rename rather than a copy through memory. A cross-device staging
        // root is the one case that needs the copy.
        if fs::rename(&upload.path, &slot.tmp_path).await.is_err() {
            fs::copy(&upload.path, &slot.tmp_path).await.map_err(RegistryError::Io)?;
            let _ = fs::remove_file(&upload.path).await;
        }
        let _ = fs::remove_file(self.uploads_root().join(upload.repository_record())).await;
        Ok(slot)
    }

    /// Reclaim uploads untouched for `max_age`, reporting how many went.
    ///
    /// Runs at startup, where it also clears whatever an unclean shutdown
    /// left behind. An upload still being written has just been appended to,
    /// so its age is its idle time rather than its lifetime.
    pub async fn sweep_blob_uploads(&self, max_age: Duration) -> Result<usize> {
        let remote_swept = match self.hosted.upload_store() {
            Some(remote) => remote.sweep(max_age).await?,
            None => 0,
        };
        let root = self.uploads_root();
        let Some(mut entries) = crate::read_dir_if_present(&root).await? else {
            return Ok(remote_swept);
        };
        let mut swept = remote_swept;
        while let Some(entry) = entries.next_entry().await.map_err(RegistryError::Io)? {
            if sweep_upload_entry(&root, &entry, max_age).await {
                swept += 1;
            }
        }
        Ok(swept)
    }

    /// Who an upload started here belongs to.
    ///
    /// The repository alone is not enough. Every organization of an
    /// object-store backend shares one uploads directory, so two of them
    /// hosting the same repository name would otherwise reach each other's
    /// uploads. The newline cannot appear in either half, so one owner has
    /// exactly one spelling.
    fn upload_owner(&self, repository: &CanonicalPackageName) -> String {
        format!("{}\n{}", self.hosted.namespace(), repository.as_str())
    }

    fn uploads_root(&self) -> PathBuf {
        self.hosted_scratch_root().join(UPLOADS_DIR)
    }
}

/// Upload ids are 32 lowercase hex characters, which is both unguessable and
/// a safe single path segment. Anything else never reaches the filesystem.
fn is_upload_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The filename holding the repository an upload was started for.
fn repository_record(id: &str) -> String {
    format!("{id}{REPOSITORY_SUFFIX}")
}

fn generate_upload_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("OS CSPRNG must be available");
    bytes.iter().fold(String::with_capacity(32), |mut hex, byte| {
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
        hex
    })
}

#[cfg(test)]
mod tests;

/// Remove one uploads-directory entry if it is stale, reporting whether an
/// upload was reclaimed.
///
/// A record has no age of its own: it goes when its upload does. One still
/// here with no upload beside it belongs to a push that stopped between the
/// two removals, and nothing else would ever reclaim it.
async fn sweep_upload_entry(root: &Path, entry: &fs::DirEntry, max_age: Duration) -> bool {
    let name = entry.file_name();
    let name = name.to_string_lossy();
    if let Some(id) = name.strip_suffix(REPOSITORY_SUFFIX) {
        if !fs::try_exists(root.join(id)).await.unwrap_or(true) {
            let _ = fs::remove_file(entry.path()).await;
        }
        return false;
    }
    let Ok(metadata) = entry.metadata().await else {
        return false;
    };
    let idle = metadata.modified().ok().and_then(|at| at.elapsed().ok());
    if idle.is_none_or(|idle| idle <= max_age) || fs::remove_file(entry.path()).await.is_err() {
        return false;
    }
    let _ = fs::remove_file(root.join(repository_record(&name))).await;
    true
}
