//! Streaming helpers for the blob path.
//!
//! Three flows live here:
//!
//! * [`stream_verified_to_cache`] streams an upstream response to the client
//!   while teeing it into the cache, promoting the entry only if the SRI
//!   matches the full body.
//! * [`download_verified_to_temp`] hashes an upstream response into a
//!   temp file for mirror-less pass-through.
//! * [`stream_file`] yields an already verified file to the response.

use crate::BlobWrite;
use axum::body::{Body, Bytes};
use futures_util::{Stream, StreamExt, stream};
use pnpm_network::ThrottledResponse;
use ssri::{Integrity, IntegrityChecker};
use std::{io, path::PathBuf, pin::Pin};
use tokio::{fs::File, io::AsyncReadExt};

/// Chunk size for reading from a cached file. 64 KiB keeps syscall
/// overhead low without buffering a meaningful fraction of a
/// multi-MB blob.
const READ_CHUNK: usize = 64 * 1024;

pub fn parse_integrity(value: &str) -> Result<Integrity, ssri::Error> {
    let integrity: Integrity = value.parse()?;
    ensure_supported_hash(&integrity)?;
    Ok(integrity)
}

fn ensure_supported_hash(integrity: &Integrity) -> Result<(), ssri::Error> {
    if integrity.hashes.is_empty() {
        return Err(ssri::Error::ParseIntegrityError(
            "integrity string contains no supported hashes".to_string(),
        ));
    }
    Ok(())
}

pub fn integrity_checker(integrity: &Integrity) -> Result<IntegrityChecker, ssri::Error> {
    ensure_supported_hash(integrity)?;
    Ok(IntegrityChecker::new(integrity.clone()))
}

#[derive(Debug)]
pub enum BlobStreamError {
    Upstream { url: String, source: io::Error },
    Io(io::Error),
    Integrity(ssri::Error),
    TooLarge { limit: u64, received: u64 },
}

/// Stream an upstream response to the client while teeing it into `write`
/// and hashing it. The client receives bytes as they arrive — it does not
/// wait for the whole download to land and verify first — and the cache entry
/// is promoted only once the declared SRI matches the full body.
///
/// Integrity failures terminate the body with a stream error and abandon the
/// temporary cache file. Headers and earlier chunks may already have reached
/// the client, so clients must still verify the received bytes. Dropping the
/// connection also abandons the temporary file through [`BlobWrite`]'s `Drop`.
pub fn stream_verified_to_cache(
    response: ThrottledResponse,
    write: BlobWrite,
    integrity: &Integrity,
    max_bytes: u64,
) -> Result<Body, BlobStreamError> {
    // Reject an upstream that already declares an oversize body up front, so it
    // surfaces as an error response instead of a failure mid-stream.
    if let Some(received) = response.content_length()
        && received > max_bytes
    {
        return Err(BlobStreamError::TooLarge { limit: max_bytes, received });
    }
    let checker = integrity_checker(integrity).map_err(BlobStreamError::Integrity)?;
    let state = TeeState {
        url: redact_url(response.url()),
        upstream: Box::pin(response.bytes_stream()),
        write: Some(write),
        checker,
        written: 0,
        max_bytes,
    };
    let body = stream::unfold(Some(state), |state| async move { next_tee_chunk(state?).await });
    Ok(Body::from_stream(body))
}

/// Advance the tee by one upstream chunk.
async fn next_tee_chunk(mut state: TeeState) -> Option<(io::Result<Bytes>, Option<TeeState>)> {
    match state.upstream.next().await {
        Some(Ok(chunk)) => forward_chunk(state, chunk).await,
        Some(Err(source)) => {
            tracing::warn!(url = %state.url, ?source, "upstream blob stream failed mid-download");
            abandon(state.write.take()).await;
            Some((Err(io::Error::other(source)), None))
        }
        None => finish_tee(state).await,
    }
}

/// Forward one chunk to the client, cache it best-effort, and count it against
/// the size limit.
async fn forward_chunk(
    mut state: TeeState,
    chunk: Bytes,
) -> Option<(io::Result<Bytes>, Option<TeeState>)> {
    let received = state.written.saturating_add(chunk.len() as u64);
    if received > state.max_bytes {
        let limit = state.max_bytes;
        tracing::warn!(
            url = %state.url,
            received,
            limit,
            "proxied blob exceeded the size limit mid-stream",
        );
        abandon(state.write.take()).await;
        return Some((Err(io::Error::other(format!("blob exceeds {limit} bytes"))), None));
    }
    state.write = cache_chunk(state.write.take(), &chunk).await;
    state.checker.input(&chunk);
    state.written = received;
    Some((Ok(chunk), Some(state)))
}

/// Write one chunk to the cache. The cache is best-effort: if the temp write
/// fails, stop caching but keep streaming to the client.
async fn cache_chunk(write: Option<BlobWrite>, chunk: &[u8]) -> Option<BlobWrite> {
    let mut write = write?;
    match write.write_all(chunk).await {
        Ok(()) => Some(write),
        Err(err) => {
            tracing::warn!(?err, "blob cache write failed; serving without caching");
            write.abandon().await;
            None
        }
    }
}

/// The upstream ended: promote the blob to the cache only if it matched the
/// integrity it was fetched under.
async fn finish_tee(mut state: TeeState) -> Option<(io::Result<Bytes>, Option<TeeState>)> {
    match state.checker.result() {
        Ok(_) => {
            finalize(state.write.take()).await;
            None
        }
        Err(err) => {
            tracing::warn!(url = %state.url, ?err, "proxied blob failed integrity; not caching it");
            abandon(state.write.take()).await;
            Some((Err(io::Error::other(err)), None))
        }
    }
}

/// Promote a fully-streamed, SRI-matched blob to the cache, logging (not
/// failing — the client already has the bytes) if the rename can't complete.
async fn finalize(write: Option<BlobWrite>) {
    if let Some(write) = write
        && let Err(err) = write.finalize().await
    {
        tracing::warn!(?err, "promoting verified blob to cache failed");
    }
}

async fn abandon(write: Option<BlobWrite>) {
    if let Some(write) = write {
        write.abandon().await;
    }
}

/// A log-safe form of the upstream URL: basic-auth userinfo and the query
/// string (which can carry presigned-redirect tokens) are stripped, since this
/// URL is only kept to tag failure logs.
fn redact_url(url: &reqwest::Url) -> String {
    let mut url = url.clone();
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.to_string()
}

/// Carries the in-flight tee through [`stream::unfold`]: the upstream byte
/// stream, the cache writer (dropped once caching is abandoned), the running
/// SRI checker, and the size budget.
struct TeeState {
    /// The upstream blob URL, kept only to tag failure logs.
    url: String,
    upstream: Pin<Box<dyn Stream<Item = io::Result<Bytes>> + Send>>,
    write: Option<BlobWrite>,
    checker: IntegrityChecker,
    written: u64,
    max_bytes: u64,
}

pub async fn download_verified_to_temp(
    response: ThrottledResponse,
    mut write: BlobWrite,
    integrity: &Integrity,
    max_bytes: u64,
) -> Result<(File, u64, PathBuf), BlobStreamError> {
    if let Err(err) = download_verified(response, &mut write, integrity, max_bytes).await {
        write.abandon().await;
        return Err(err);
    }
    write.into_temp_file().await.map_err(BlobStreamError::Io)
}

async fn download_verified(
    response: ThrottledResponse,
    write: &mut BlobWrite,
    integrity: &Integrity,
    max_bytes: u64,
) -> Result<u64, BlobStreamError> {
    let url = response.url().to_string();
    if let Some(received) = response.content_length()
        && received > max_bytes
    {
        return Err(BlobStreamError::TooLarge { limit: max_bytes, received });
    }
    let mut upstream = Box::pin(response.bytes_stream());
    let mut checker = integrity_checker(integrity).map_err(BlobStreamError::Integrity)?;
    let mut written = 0u64;
    while let Some(chunk_result) = upstream.next().await {
        let chunk = match chunk_result {
            Ok(chunk) => chunk,
            Err(source) => return Err(BlobStreamError::Upstream { url, source }),
        };
        let received = written.saturating_add(chunk.len() as u64);
        if received > max_bytes {
            return Err(BlobStreamError::TooLarge { limit: max_bytes, received });
        }
        if let Err(err) = write.write_all(&chunk).await {
            return Err(BlobStreamError::Io(err));
        }
        checker.input(&chunk);
        written = received;
    }

    if let Err(err) = checker.result() {
        return Err(BlobStreamError::Integrity(err));
    }
    Ok(written)
}

/// Stream a cached file as a response body. Caller is responsible for
/// setting `Content-Length` (from the file metadata it already read).
pub fn stream_file(file: impl tokio::io::AsyncRead + Unpin + Send + 'static) -> Body {
    // Carry the `File` through the unfold *state* (not as a closure
    // capture) so each step owns it, reads, and hands it back. An
    // `FnMut` closure can't move the file across iterations on its
    // own.
    let stream = stream::unfold(Some(file), |state| async move {
        let mut file = state?;
        let mut buf = vec![0u8; READ_CHUNK];
        match file.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok::<_, io::Error>(Bytes::from(buf)), Some(file)))
            }
            Err(err) => Some((Err(err), None)),
        }
    });
    Body::from_stream(stream)
}

pub fn stream_file_and_remove(file: File, path: PathBuf) -> Body {
    let stream = stream::unfold(Some(RemoveOnDropFile::new(file, path)), |state| async move {
        let mut state = state?;
        let mut buf = vec![0u8; READ_CHUNK];
        let file = state.file.as_mut().expect("file is present until stream finishes");
        match file.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok::<_, io::Error>(Bytes::from(buf)), Some(state)))
            }
            Err(err) => Some((Err(err), None)),
        }
    });
    Body::from_stream(stream)
}

struct RemoveOnDropFile {
    file: Option<File>,
    path: Option<PathBuf>,
}

impl RemoveOnDropFile {
    fn new(file: File, path: PathBuf) -> Self {
        Self { file: Some(file), path: Some(path) }
    }
}

impl Drop for RemoveOnDropFile {
    fn drop(&mut self) {
        drop(self.file.take());
        let Some(path) = self.path.take() else { return };
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => {
                tracing::warn!(?err, path = %path.display(), "temporary blob cleanup failed");
            }
        }
    }
}

#[cfg(test)]
mod tests;
