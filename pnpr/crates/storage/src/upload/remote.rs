use super::{BlobUpload, BlobUploadWriter, generate_upload_id, is_upload_id};
use crate::s3::send_parts;
use futures_util::StreamExt;
use object_store::{ObjectStore, ObjectStoreExt, PutMode, PutOptions, UpdateVersion, path::Path};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::CanonicalPackageName;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Duration};
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
        state.record.closed = true;
        self.write(id, &state.record, PutMode::Update(state.version)).await?;
        self.remove_chunks(id, &state.record).await?;
        Ok(true)
    }

    async fn remove_chunks(&self, id: &str, record: &UploadRecord) -> Result<()> {
        for chunk in &record.chunks {
            self.remove(&self.key(id, chunk)).await?;
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
        let mut live_sessions = HashSet::new();
        while let Some(meta) = listing.next().await {
            let meta = meta?;
            let Some(relative) = meta.location.as_ref().strip_prefix(&self.prefix) else {
                continue;
            };
            let Some((id, object)) = relative.split_once('/') else { continue };
            if !is_upload_id(id) {
                continue;
            }
            let age = std::time::SystemTime::from(meta.last_modified).elapsed().unwrap_or_default();
            if age <= max_age || (object != "session.json" && live_sessions.contains(id)) {
                continue;
            }
            let Some(mut state) = self.read(id).await? else {
                self.remove(&meta.location).await?;
                continue;
            };
            if object == "session.json" {
                // The listing may predate a successful append. Compare the
                // listed version, never a freshly read version, when expiring it.
                state.record.closed = true;
                let version = UpdateVersion { e_tag: meta.e_tag, version: meta.version };
                match self.write(id, &state.record, PutMode::Update(version)).await {
                    Ok(_) => {}
                    Err(RegistryError::BlobUploadConflict { .. }) => continue,
                    Err(error) => return Err(error),
                }
                live_sessions.remove(id);
                self.remove_chunks(id, &state.record).await?;
                self.remove(&meta.location).await?;
                swept += 1;
            } else if state.record.closed {
                self.remove(&meta.location).await?;
            } else {
                live_sessions.insert(id.to_string());
            }
        }
        Ok(swept)
    }
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

    pub(super) async fn close(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        let mut record = state.record.clone();
        record.closed = true;
        let version =
            self.backend.write(&self.id, &record, PutMode::Update(state.version.clone())).await?;
        *state = VersionedRecord { record, version };
        if let Err(error) = self.backend.remove_chunks(&self.id, &state.record).await {
            tracing::warn!(%error, upload = self.id, "completed upload chunks await expiry cleanup");
        }
        Ok(())
    }
}

impl RemoteChunk {
    pub(super) async fn commit(mut self, size: u64) -> Result<u64> {
        if size == 0 {
            return Ok(self.snapshot.size);
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
