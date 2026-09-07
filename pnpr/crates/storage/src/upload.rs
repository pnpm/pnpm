//! Resumable blob uploads.
//!
//! A blob whose bytes arrive across several requests cannot be held in
//! memory between them, so an upload is a local file the client appends to
//! and an id that names it. Only when the client finishes and the bytes hash
//! to what it promised does the file become a hosted blob.
//!
//! The file lives beside the publish journal in the backend's local scratch
//! root, so an S3-backed registry stages here too and the eventual promotion
//! is one rename or one upload rather than a second copy through memory.

use crate::{BlobSlot, Storage};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::CanonicalPackageName;
use std::{fmt::Write as _, path::PathBuf, time::Duration};
use tokio::{
    fs,
    io::{AsyncWriteExt, ErrorKind},
};

/// Where in-progress uploads live under the backend's local scratch root.
/// The dot prefix keeps it out of the hosted store's package walk.
pub(crate) const UPLOADS_DIR: &str = ".pnpr-uploads";

/// Where an upload records the repository it was started for. The id alone
/// would let a caller who learns one finish it into a different repository of
/// the same organization, with bytes they never sent.
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
        match fs::metadata(&self.path).await {
            Ok(metadata) => Ok(metadata.len()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(0),
            Err(error) => Err(RegistryError::Io(error)),
        }
    }

    /// Where the accumulated bytes are, for a caller that needs to read them
    /// back without holding them in memory.
    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Open the upload for appending. Held for one request, so a chunk is
    /// written with one open rather than one per buffer.
    pub async fn append(&self) -> Result<BlobUploadWriter> {
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await
            .map_err(RegistryError::Io)?;
        Ok(BlobUploadWriter { file })
    }
}

/// The append handle for one request's worth of an upload.
pub struct BlobUploadWriter {
    file: fs::File,
}

impl BlobUploadWriter {
    pub async fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        self.file.write_all(bytes).await.map_err(RegistryError::Io)
    }

    /// Flush to disk and report the upload's new length.
    pub async fn finish(self) -> Result<u64> {
        self.file.sync_all().await.map_err(RegistryError::Io)?;
        self.file.metadata().await.map(|metadata| metadata.len()).map_err(RegistryError::Io)
    }
}

impl Storage {
    /// Start an upload for `repository` and give it an unguessable id.
    pub async fn begin_blob_upload(&self, repository: &CanonicalPackageName) -> Result<BlobUpload> {
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
        fs::write(root.join(format!("{id}{REPOSITORY_SUFFIX}")), repository.as_str())
            .await
            .map_err(RegistryError::Io)?;
        Ok(BlobUpload { id, path })
    }

    /// Reopen an upload of `repository` by id.
    ///
    /// `None` when no such upload is held, and equally when one is held for
    /// another repository: an id is a capability over the bytes its own
    /// client sent, not over the organization's scratch space.
    pub async fn open_blob_upload(
        &self,
        repository: &CanonicalPackageName,
        id: &str,
    ) -> Result<Option<BlobUpload>> {
        if !is_upload_id(id) {
            return Ok(None);
        }
        let root = self.uploads_root();
        let held = match fs::read_to_string(root.join(format!("{id}{REPOSITORY_SUFFIX}"))).await {
            Ok(held) => held,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(RegistryError::Io(error)),
        };
        if held != repository.as_str() {
            return Ok(None);
        }
        let path = root.join(id);
        match fs::metadata(&path).await {
            Ok(_) => Ok(Some(BlobUpload { id: id.to_string(), path })),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(RegistryError::Io(error)),
        }
    }

    /// Discard an upload, reporting whether one was held.
    pub async fn abort_blob_upload(&self, id: &str) -> Result<bool> {
        if !is_upload_id(id) {
            return Ok(false);
        }
        let root = self.uploads_root();
        let _ = fs::remove_file(root.join(format!("{id}{REPOSITORY_SUFFIX}"))).await;
        match fs::remove_file(root.join(id)).await {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(error) => Err(RegistryError::Io(error)),
        }
    }

    /// Move a finished upload into a hosted blob slot, ready for
    /// [`Storage::finalize_blob_slot`].
    ///
    /// The upload is consumed on success, where its bytes become the slot's.
    /// It is left where it was on failure, so a caller can retry or abort it
    /// rather than lose bytes a client already sent.
    pub async fn stage_uploaded_blob(
        &self,
        upload: BlobUpload,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<BlobSlot> {
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
        Ok(slot)
    }

    /// Reclaim uploads untouched for `max_age`, reporting how many went.
    ///
    /// Runs at startup, where it also clears whatever an unclean shutdown
    /// left behind. An upload still being written has just been appended to,
    /// so its age is its idle time rather than its lifetime.
    pub async fn sweep_blob_uploads(&self, max_age: Duration) -> Result<usize> {
        let root = self.uploads_root();
        let mut entries = match fs::read_dir(&root).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(RegistryError::Io(error)),
        };
        let mut swept = 0;
        while let Some(entry) = entries.next_entry().await.map_err(RegistryError::Io)? {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // The record of which repository an upload belongs to goes when
            // that upload does, rather than on an age of its own.
            if name.ends_with(REPOSITORY_SUFFIX) {
                continue;
            }
            let Ok(metadata) = entry.metadata().await else { continue };
            let idle = metadata.modified().ok().and_then(|at| at.elapsed().ok());
            if idle.is_some_and(|idle| idle > max_age)
                && fs::remove_file(entry.path()).await.is_ok()
            {
                let _ = fs::remove_file(root.join(format!("{name}{REPOSITORY_SUFFIX}"))).await;
                swept += 1;
            }
        }
        Ok(swept)
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
