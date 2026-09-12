use super::{BlobUpload, BlobUploadWriter, generate_upload_id, is_upload_id};
use crate::s3::send_parts;
use futures_util::StreamExt;
use object_store::{ObjectStore, ObjectStoreExt, PutMode, PutOptions, UpdateVersion, path::Path};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::CanonicalPackageName;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tempfile::TempPath;
use tokio::{fs, io::AsyncWriteExt, sync::Mutex};

const MAX_CHUNKS: usize = 10_000;

#[derive(Debug, Clone)]
pub(crate) struct RemoteUploadStore {
    store: Arc<dyn ObjectStore>,
    prefix: String,
    scratch: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UploadRecord {
    repository: String,
    chunks: Vec<String>,
    size: u64,
    closed: bool,
    #[serde(default)]
    completion: Option<String>,
}

#[derive(Debug)]
struct VersionedRecord {
    record: UploadRecord,
    version: UpdateVersion,
}

#[derive(Debug)]
pub(super) struct RemoteUpload {
    backend: RemoteUploadStore,
    id: String,
    state: Mutex<VersionedRecord>,
}

pub(super) struct RemoteChunk {
    upload: Arc<RemoteUpload>,
    snapshot: UploadRecord,
    version: UpdateVersion,
    path: TempPath,
}

impl RemoteUploadStore {
    pub(crate) fn new(store: Arc<dyn ObjectStore>, prefix: &str, scratch: PathBuf) -> Self {
        Self { store, prefix: format!("{prefix}.pnpr-uploads/"), scratch }
    }

    fn key(&self, id: &str, object: &str) -> Path {
        Path::from(format!("{}{id}/{object}", self.prefix))
    }

    async fn read(&self, id: &str) -> Result<Option<VersionedRecord>> {
        let result = match self.store.get(&self.key(id, "session.json")).await {
            Ok(result) => result,
            Err(object_store::Error::NotFound { .. }) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let version = UpdateVersion {
            e_tag: result.meta.e_tag.clone(),
            version: result.meta.version.clone(),
        };
        let record: UploadRecord = serde_json::from_slice(&result.bytes().await?)?;
        if record.chunks.len() > MAX_CHUNKS || record.chunks.iter().any(|id| !is_upload_id(id)) {
            return Err(RegistryError::Internal { reason: "invalid upload chunk list".into() });
        }
        Ok(Some(VersionedRecord { record, version }))
    }

    async fn write(&self, id: &str, record: &UploadRecord, mode: PutMode) -> Result<UpdateVersion> {
        let result = self
            .store
            .put_opts(
                &self.key(id, "session.json"),
                serde_json::to_vec(record)?.into(),
                PutOptions { mode, ..Default::default() },
            )
            .await
            .map_err(|error| match error {
                object_store::Error::Precondition { .. }
                | object_store::Error::AlreadyExists { .. }
                | object_store::Error::NotFound { .. } => {
                    RegistryError::BlobUploadConflict { id: id.to_string() }
                }
                error => error.into(),
            })?;
        Ok(UpdateVersion { e_tag: result.e_tag, version: result.version })
    }

    async fn temp(&self) -> Result<TempPath> {
        fs::create_dir_all(&self.scratch).await?;
        Ok(tempfile::NamedTempFile::new_in(&self.scratch)?.into_temp_path())
    }

    async fn handle(&self, id: &str, state: VersionedRecord) -> Result<BlobUpload> {
        let temp = Arc::new(self.temp().await?);
        Ok(BlobUpload {
            id: id.to_string(),
            path: temp.to_path_buf(),
            remote: Some(Arc::new(RemoteUpload {
                backend: self.clone(),
                id: id.to_string(),
                state: Mutex::new(state),
            })),
            _temp: Some(temp),
        })
    }

    pub(super) async fn begin(&self, repository: &CanonicalPackageName) -> Result<BlobUpload> {
        let id = generate_upload_id();
        let record = UploadRecord {
            repository: repository.as_str().to_string(),
            chunks: Vec::new(),
            size: 0,
            closed: false,
            completion: None,
        };
        let version = self.write(&id, &record, PutMode::Create).await?;
        self.handle(&id, VersionedRecord { record, version }).await
    }

    pub(super) async fn open(
        &self,
        repository: &CanonicalPackageName,
        id: &str,
    ) -> Result<Option<BlobUpload>> {
        let Some(state) = self.read(id).await? else { return Ok(None) };
        if state.record.closed || state.record.repository != repository.as_str() {
            return Ok(None);
        }
        self.handle(id, state).await.map(Some)
    }

    pub(super) async fn abort(&self, id: &str) -> Result<bool> {
        let Some(mut state) = self.read(id).await? else { return Ok(false) };
        if state.record.closed {
            return Ok(false);
        }
        if state.record.completion.is_some() {
            return Err(RegistryError::BlobUploadConflict { id: id.to_string() });
        }
        state.record.closed = true;
        self.write(id, &state.record, PutMode::Update(state.version)).await?;
        self.remove_chunks(id).await?;
        Ok(true)
    }

    async fn remove_chunks(&self, id: &str) -> Result<()> {
        let prefix = Path::from(format!("{}{id}/", self.prefix));
        let mut listing = self.store.list(Some(&prefix));
        let session = self.key(id, "session.json");
        while let Some(meta) = listing.next().await {
            let meta = meta?;
            if meta.location != session {
                self.remove(&meta.location).await?;
            }
        }
        Ok(())
    }

    async fn remove(&self, path: &Path) -> Result<()> {
        match self.store.delete(path).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub(super) async fn sweep(&self, max_age: Duration) -> Result<usize> {
        let mut listing = self.store.list(Some(&Path::from(self.prefix.clone())));
        let mut swept = 0;
        let mut state = SweepState::default();
        while let Some(meta) = listing.next().await {
            let meta = meta?;
            let Some(relative) = meta.location.as_ref().strip_prefix(&self.prefix) else {
                continue;
            };
            let Some((id, object)) = relative.split_once('/') else { continue };
            let age = std::time::SystemTime::from(meta.last_modified).elapsed().unwrap_or_default();
            if !is_upload_id(id) || age <= max_age || state.unreadable.contains(id) {
                continue;
            }
            if self.expire_upload_object(id, object, &meta, &mut state).await? {
                swept += 1;
            }
        }
        Ok(swept)
    }

    /// Expire one listed object of an upload no longer being written to,
    /// reporting whether the upload itself was reclaimed.
    async fn expire_upload_object(
        &self,
        id: &str,
        object: &str,
        meta: &object_store::ObjectMeta,
        state: &mut SweepState,
    ) -> Result<bool> {
        if object == "session.json" {
            return self.expire_upload_session(id, meta, state).await;
        }
        self.expire_upload_chunk(id, meta, state).await?;
        Ok(false)
    }

    /// Drop a chunk whose session is already closed or gone. A chunk of a live
    /// session stays: the push may still be running.
    async fn expire_upload_chunk(
        &self,
        id: &str,
        meta: &object_store::ObjectMeta,
        state: &mut SweepState,
    ) -> Result<()> {
        if let Some(live) = state.sessions.get(id) {
            if !live {
                self.remove(&meta.location).await?;
            }
            return Ok(());
        }
        let record = match self.sweep_session(id, state).await? {
            SweepSession::Record(record) => record,
            SweepSession::Missing => {
                state.sessions.insert(id.to_string(), false);
                self.remove(&meta.location).await?;
                return Ok(());
            }
            SweepSession::Unreadable => return Ok(()),
        };
        state.sessions.insert(id.to_string(), !record.record.closed);
        if record.record.closed {
            self.remove(&meta.location).await?;
        }
        Ok(())
    }

    /// Close an expired session and reclaim what it left behind.
    async fn expire_upload_session(
        &self,
        id: &str,
        meta: &object_store::ObjectMeta,
        state: &mut SweepState,
    ) -> Result<bool> {
        let mut record = match self.sweep_session(id, state).await? {
            SweepSession::Record(record) => record,
            SweepSession::Missing => {
                state.sessions.insert(id.to_string(), false);
                self.remove(&meta.location).await?;
                return Ok(false);
            }
            SweepSession::Unreadable => return Ok(false),
        };
        // The listing may predate a successful append. Compare the listed
        // version, never a freshly read version, when expiring it.
        record.record.closed = true;
        let version = UpdateVersion { e_tag: meta.e_tag.clone(), version: meta.version.clone() };
        match self.write(id, &record.record, PutMode::Update(version)).await {
            Ok(_) => {}
            Err(RegistryError::BlobUploadConflict { .. }) => return Ok(false),
            Err(error) => return Err(error),
        }
        state.sessions.insert(id.to_string(), false);
        self.remove_chunks(id).await?;
        self.remove(&meta.location).await?;
        Ok(true)
    }

    /// Read an upload's session record, remembering an unreadable one so the
    /// rest of the sweep leaves that upload alone.
    async fn sweep_session(&self, id: &str, state: &mut SweepState) -> Result<SweepSession> {
        match self.read(id).await {
            Ok(Some(record)) => Ok(SweepSession::Record(record)),
            Ok(None) => Ok(SweepSession::Missing),
            Err(error) => {
                tracing::warn!(error = %error.log_message(), upload = id, "skipping unreadable upload session");
                state.unreadable.insert(id.to_string());
                Ok(SweepSession::Unreadable)
            }
        }
    }
}

/// What one sweep has learned about the uploads it has listed so far.
#[derive(Default)]
struct SweepState {
    /// Whether an upload id's session is still open.
    sessions: HashMap<String, bool>,
    /// Upload ids whose session object could not be read this sweep.
    unreadable: HashSet<String>,
}

/// An upload's session as this sweep found it.
enum SweepSession {
    Record(VersionedRecord),
    /// The session object is gone, so anything left under the id is orphaned.
    Missing,
    /// The session could not be read; leave the whole upload alone.
    Unreadable,
}

impl RemoteUpload {
    pub(super) async fn offset(&self) -> u64 {
        self.state.lock().await.record.size
    }

    pub(super) async fn append(self: &Arc<Self>) -> Result<BlobUploadWriter> {
        let state = self.state.lock().await;
        let path = self.backend.temp().await?;
        let file = fs::OpenOptions::new().write(true).open(&path).await?;
        Ok(BlobUploadWriter {
            file,
            remote: Some(RemoteChunk {
                upload: Arc::clone(self),
                snapshot: state.record.clone(),
                version: state.version.clone(),
                path,
            }),
        })
    }

    pub(super) async fn materialize(&self, path: &std::path::Path) -> Result<()> {
        let state = self.state.lock().await;
        if fs::metadata(path).await?.len() == state.record.size {
            return Ok(());
        }
        let mut file = fs::File::create(path).await?;
        for chunk in &state.record.chunks {
            let mut stream =
                self.backend.store.get(&self.backend.key(&self.id, chunk)).await?.into_stream();
            while let Some(bytes) = stream.next().await {
                file.write_all(&bytes?).await?;
            }
        }
        file.sync_all().await?;
        Ok(())
    }

    /// Freeze the verified chunk list before promotion. A retry can promote the
    /// same digest after a crash or failed object-store write.
    pub(super) async fn prepare_completion(&self, filename: &str) -> Result<()> {
        let mut state = self.state.lock().await;
        if state.record.closed
            || state.record.completion.as_deref().is_some_and(|value| value != filename)
        {
            return Err(RegistryError::BlobUploadConflict { id: self.id.clone() });
        }
        let mut record = state.record.clone();
        record.completion = Some(filename.to_string());
        let version =
            self.backend.write(&self.id, &record, PutMode::Update(state.version.clone())).await?;
        *state = VersionedRecord { record, version };
        Ok(())
    }

    pub(super) async fn close(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        let mut record = state.record.clone();
        record.closed = true;
        let version =
            self.backend.write(&self.id, &record, PutMode::Update(state.version.clone())).await?;
        *state = VersionedRecord { record, version };
        if let Err(error) = self.backend.remove_chunks(&self.id).await {
            tracing::warn!(%error, upload = self.id, "completed upload chunks await expiry cleanup");
        }
        Ok(())
    }
}

impl RemoteChunk {
    pub(super) async fn commit(mut self, size: u64) -> Result<u64> {
        if size == 0 {
            let current = self.upload.backend.read(&self.upload.id).await?;
            if current
                .is_none_or(|current| current.record.closed || current.version != self.version)
            {
                return Err(RegistryError::BlobUploadConflict { id: self.upload.id.clone() });
            }
            return Ok(self.snapshot.size);
        }
        if self.snapshot.closed || self.snapshot.completion.is_some() {
            return Err(RegistryError::BlobUploadConflict { id: self.upload.id.clone() });
        }
        if self.snapshot.chunks.len() == MAX_CHUNKS {
            return Err(RegistryError::BadRequest { reason: "upload has too many chunks".into() });
        }
        let chunk = generate_upload_id();
        let key = self.upload.backend.key(&self.upload.id, &chunk);
        let mut multipart = self.upload.backend.store.put_multipart(&key).await?;
        send_parts(&self.path, multipart.as_mut()).await?;
        self.snapshot.chunks.push(chunk);
        self.snapshot.size =
            self.snapshot.size.checked_add(size).ok_or_else(|| RegistryError::BadRequest {
                reason: "upload size overflow".into(),
            })?;
        // An uncertain write may have committed. Leave its chunk until session
        // expiry, when no append can make the chunk reachable anymore.
        let version = self
            .upload
            .backend
            .write(&self.upload.id, &self.snapshot, PutMode::Update(self.version))
            .await?;
        let offset = self.snapshot.size;
        *self.upload.state.lock().await = VersionedRecord { record: self.snapshot, version };
        Ok(offset)
    }
}

#[cfg(test)]
mod tests;
