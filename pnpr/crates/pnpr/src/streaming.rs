//! Streaming helpers for the tarball path.
//!
//! Three flows live here:
//!
//! * [`stream_verified_to_cache`] streams an upstream response to the client
//!   while teeing it into the cache, promoting the entry only if the SRI
//!   matches the full body.
//! * [`download_verified_to_temp`] hashes an upstream response into a
//!   temp file for mirror-less pass-through.
//! * [`stream_file`] yields an already verified file to the response.

use crate::storage::TarballWrite;
use axum::body::{Body, Bytes};
use futures_util::{Stream, StreamExt, stream};
use ssri::{Integrity, IntegrityChecker};
use std::{io, path::PathBuf, pin::Pin};
use tokio::{fs::File, io::AsyncReadExt};

/// Chunk size for reading from a cached file. 64 KiB keeps syscall
/// overhead low without buffering a meaningful fraction of a
/// multi-MB tarball.
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
pub enum TarballStreamError {
    Upstream { url: String, source: reqwest::Error },
    Io(io::Error),
    Integrity(ssri::Error),
    TooLarge { limit: u64, received: u64 },
}

/// Stream an upstream response to the client while teeing it into `write`
/// and hashing it. The client receives bytes as they arrive — it does not
/// wait for the whole download to land and verify first — and the cache entry
/// is promoted only once the declared SRI matches the full body.
///
/// SRI can only be checked after the last byte, by which point the body has
/// already been streamed, so a mismatch can't be turned into an error
/// response. The guarantees that remain are the ones that matter: a
/// mismatched (or truncated, or oversize) body is never promoted to the cache,
/// so it can't poison a future client, and every install client re-verifies
/// what it received against its own expected integrity and rejects bad bytes.
/// On any such failure — or a dropped client connection — the temp file is
/// abandoned (and [`TarballWrite`]'s `Drop` removes it as a backstop).
pub fn stream_verified_to_cache(
    response: reqwest::Response,
    write: TarballWrite,
    integrity: &Integrity,
    max_bytes: u64,
) -> Result<Body, TarballStreamError> {
    // Reject an upstream that already declares an oversize body up front, so it
    // surfaces as an error response instead of a failure mid-stream.
    if let Some(received) = response.content_length()
        && received > max_bytes
    {
        return Err(TarballStreamError::TooLarge { limit: max_bytes, received });
    }
    let checker = integrity_checker(integrity).map_err(TarballStreamError::Integrity)?;
    let state = TeeState {
        url: response.url().to_string(),
        upstream: Box::pin(response.bytes_stream()),
        write: Some(write),
        checker,
        written: 0,
        max_bytes,
    };
    let body = stream::unfold(Some(state), |state| async move {
        let mut state = state?;
        match state.upstream.next().await {
            Some(Ok(chunk)) => {
                let received = state.written.saturating_add(chunk.len() as u64);
                if received > state.max_bytes {
                    let limit = state.max_bytes;
                    tracing::warn!(
                        url = %state.url,
                        received,
                        limit,
                        "proxied tarball exceeded the size limit mid-stream",
                    );
                    abandon(state.write.take()).await;
                    return Some((
                        Err(io::Error::other(format!("tarball exceeds {limit} bytes"))),
                        None,
                    ));
                }
                if let Some(mut write) = state.write.take() {
                    // The cache is best-effort: if the temp write fails, stop
                    // caching but keep streaming to the client.
                    match write.write_all(&chunk).await {
                        Ok(()) => state.write = Some(write),
                        Err(err) => {
                            tracing::warn!(
                                ?err,
                                "tarball cache write failed; serving without caching",
                            );
                            write.abandon().await;
                        }
                    }
                }
                state.checker.input(&chunk);
                state.written = received;
                Some((Ok(chunk), Some(state)))
            }
            Some(Err(source)) => {
                tracing::warn!(url = %state.url, ?source, "upstream tarball stream failed mid-download");
                abandon(state.write.take()).await;
                Some((Err(io::Error::other(source)), None))
            }
            None => {
                match state.checker.result() {
                    Ok(_) => finalize(state.write.take()).await,
                    Err(err) => {
                        tracing::warn!(url = %state.url, ?err, "proxied tarball failed integrity; not caching it");
                        abandon(state.write.take()).await;
                    }
                }
                None
            }
        }
    });
    Ok(Body::from_stream(body))
}

/// Promote a fully-streamed, SRI-matched tarball to the cache, logging (not
/// failing — the client already has the bytes) if the rename can't complete.
async fn finalize(write: Option<TarballWrite>) {
    if let Some(write) = write
        && let Err(err) = write.finalize().await
    {
        tracing::warn!(?err, "promoting verified tarball to cache failed");
    }
}

async fn abandon(write: Option<TarballWrite>) {
    if let Some(write) = write {
        write.abandon().await;
    }
}

/// Carries the in-flight tee through [`stream::unfold`]: the upstream byte
/// stream, the cache writer (dropped once caching is abandoned), the running
/// SRI checker, and the size budget.
struct TeeState {
    /// The upstream tarball URL, kept only to tag failure logs.
    url: String,
    upstream: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    write: Option<TarballWrite>,
    checker: IntegrityChecker,
    written: u64,
    max_bytes: u64,
}

pub async fn download_verified_to_temp(
    response: reqwest::Response,
    mut write: TarballWrite,
    integrity: &Integrity,
    max_bytes: u64,
) -> Result<(File, u64, PathBuf), TarballStreamError> {
    if let Err(err) = download_verified(response, &mut write, integrity, max_bytes).await {
        write.abandon().await;
        return Err(err);
    }
    write.into_temp_file().await.map_err(TarballStreamError::Io)
}

async fn download_verified(
    response: reqwest::Response,
    write: &mut TarballWrite,
    integrity: &Integrity,
    max_bytes: u64,
) -> Result<u64, TarballStreamError> {
    let url = response.url().to_string();
    if let Some(received) = response.content_length()
        && received > max_bytes
    {
        return Err(TarballStreamError::TooLarge { limit: max_bytes, received });
    }
    let mut upstream = response.bytes_stream();
    let mut checker = integrity_checker(integrity).map_err(TarballStreamError::Integrity)?;
    let mut written = 0u64;
    while let Some(chunk_result) = upstream.next().await {
        let chunk = match chunk_result {
            Ok(chunk) => chunk,
            Err(source) => return Err(TarballStreamError::Upstream { url, source }),
        };
        let received = written.saturating_add(chunk.len() as u64);
        if received > max_bytes {
            return Err(TarballStreamError::TooLarge { limit: max_bytes, received });
        }
        if let Err(err) = write.write_all(&chunk).await {
            return Err(TarballStreamError::Io(err));
        }
        checker.input(&chunk);
        written = received;
    }

    if let Err(err) = checker.result() {
        return Err(TarballStreamError::Integrity(err));
    }
    Ok(written)
}

/// Stream a cached file as a response body. Caller is responsible for
/// setting `Content-Length` (from the file metadata it already read).
pub fn stream_file(file: File) -> Body {
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
                tracing::warn!(?err, path = %path.display(), "temporary tarball cleanup failed");
            }
        }
    }
}

#[cfg(test)]
mod tests;
