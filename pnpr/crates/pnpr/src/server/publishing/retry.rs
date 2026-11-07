use axum::body::Bytes;
use pnpr_error::RegistryError;
use std::{future::Future, io::Write as _};
use tokio::io::{AsyncReadExt as _, AsyncSeekExt as _};

/// The most request bytes retained in memory between publish attempts.
const MAX_MEMORY_REPLAY_BYTES: usize = 1024 * 1024;

/// Retry a request whose archives a concurrent reclaim fenced off. Each
/// attempt must validate and stage the original request again, including its
/// authorization and tombstone checks, before choosing new incarnations.
/// The attempt must release its body after parsing, before staging.
pub(in super::super) async fn retry_reclaimed<
    Output,
    Attempt: Future<Output = Result<Output, RegistryError>>,
>(
    body: Bytes,
    mut attempt: impl FnMut(Bytes) -> Attempt,
) -> Result<Output, RegistryError> {
    let mut replay = ReplayBody::new(&body).await?;
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
    Memory(Bytes),
    File { file: tokio::fs::File, length: usize },
}

impl ReplayBody {
    async fn new(body: &Bytes) -> Result<Self, RegistryError> {
        if body.len() <= MAX_MEMORY_REPLAY_BYTES {
            return Ok(Self::Memory(body.clone()));
        }
        let body = body.clone();
        tokio::task::spawn_blocking(move || {
            let mut file = tempfile::tempfile()?;
            file.write_all(&body)?;
            let file = tokio::fs::File::from_std(file);
            Ok(Self::File { file, length: body.len() })
        })
        .await?
    }

    async fn read(&mut self) -> Result<Bytes, RegistryError> {
        match self {
            Self::Memory(body) => Ok(body.clone()),
            Self::File { file, length } => {
                file.rewind().await?;
                let mut bytes = Vec::with_capacity(*length);
                file.read_to_end(&mut bytes).await?;
                Ok(Bytes::from(bytes))
            }
        }
    }
}

#[cfg(test)]
mod tests;
