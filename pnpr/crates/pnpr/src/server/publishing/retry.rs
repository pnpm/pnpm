use axum::body::Bytes;
use pnpr_error::RegistryError;
use std::{
    fs::File,
    future::Future,
    io::{Read as _, Seek as _, Write as _},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// The request size in bytes above which replay uses the cache.
const MEMORY_REPLAY_THRESHOLD: usize = 1024 * 1024;

/// The most request bytes held for large-request replays by one server instance.
const MAX_LARGE_REPLAY_BYTES: usize = 4 * super::super::MAX_PUBLISH_BODY_BYTES;

/// Large-request replays in the configured cache, admitted by total byte size.
pub(in super::super) struct ReplayStorage {
    directory: PathBuf,
    bytes: Arc<Semaphore>,
}

impl ReplayStorage {
    pub(in super::super) fn new(cache_dir: &Path) -> Self {
        Self {
            directory: cache_dir.join(".publish-replay"),
            bytes: Arc::new(Semaphore::new(MAX_LARGE_REPLAY_BYTES)),
        }
    }

    fn reserve(&self, length: usize) -> Result<OwnedSemaphorePermit, RegistryError> {
        let length = u32::try_from(length).map_err(|_| replay_capacity())?;
        Arc::clone(&self.bytes).try_acquire_many_owned(length).map_err(|_| replay_capacity())
    }
}

fn replay_capacity() -> RegistryError {
    RegistryError::TooManyRequests {
        reason: "Too many large publishes are in progress; try again later".to_string(),
    }
}

/// Retry a request whose archives a concurrent reclaim fenced off. Each
/// attempt must validate and stage the original request again, including its
/// authorization and tombstone checks, before choosing new incarnations.
/// The attempt must release its body after parsing, before staging.
pub(in super::super) async fn retry_reclaimed<
    Output,
    Attempt: Future<Output = Result<Output, RegistryError>>,
>(
    storage: &ReplayStorage,
    body: Bytes,
    mut attempt: impl FnMut(Bytes) -> Attempt,
) -> Result<Output, RegistryError> {
    let mut replay = ReplayBody::new(storage, &body).await?;
    let mut outcome = attempt(body).await;
    for _ in 0..4 {
        if !matches!(outcome, Err(RegistryError::ArchiveReclaimed { .. })) {
            return outcome;
        }
        outcome = attempt(replay.read().await?).await;
    }
    outcome
}

enum ReplayBody {
    Memory { body: Bytes, _permit: Option<OwnedSemaphorePermit> },
    File { file: Arc<ReplayFile>, length: usize },
}

impl ReplayBody {
    async fn new(storage: &ReplayStorage, body: &Bytes) -> Result<Self, RegistryError> {
        if body.len() <= MEMORY_REPLAY_THRESHOLD {
            return Ok(Self::Memory { body: body.clone(), _permit: None });
        }
        let permit = storage.reserve(body.len())?;
        let directory = storage.directory.clone();
        let write_body = body.clone();
        let (stored, permit) = tokio::task::spawn_blocking(move || {
            let stored = write_replay(&directory, &write_body);
            (stored, permit)
        })
        .await?;
        match stored {
            Ok(file) => Ok(Self::File {
                file: Arc::new(ReplayFile { file: Mutex::new(file), _permit: permit }),
                length: body.len(),
            }),
            Err(error) => {
                tracing::warn!(
                    directory = %storage.directory.display(),
                    %error,
                    "Could not write publish request to cache; using memory for retries",
                );
                Ok(Self::Memory { body: body.clone(), _permit: Some(permit) })
            }
        }
    }

    async fn read(&mut self) -> Result<Bytes, RegistryError> {
        match self {
            Self::Memory { body, .. } => Ok(body.clone()),
            Self::File { file, length } => {
                let file = Arc::clone(file);
                let length = *length;
                let bytes = tokio::task::spawn_blocking(move || file.read_bytes(length)).await??;
                Ok(Bytes::from(bytes))
            }
        }
    }
}

struct ReplayFile {
    file: Mutex<File>,
    _permit: OwnedSemaphorePermit,
}

impl ReplayFile {
    fn read_bytes(&self, length: usize) -> std::io::Result<Vec<u8>> {
        let mut file = self.file.lock().expect("the replay file lock was poisoned");
        file.rewind()?;
        let mut bytes = vec![0; length];
        file.read_exact(&mut bytes)?;
        Ok(bytes)
    }
}

fn write_replay(directory: &Path, body: &[u8]) -> std::io::Result<File> {
    std::fs::create_dir_all(directory)?;
    let mut file = tempfile::tempfile_in(directory)?;
    file.write_all(body)?;
    Ok(file)
}

#[cfg(test)]
mod tests;
