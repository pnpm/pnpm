//! Streaming helpers for the tarball path.
//!
//! Flows that live here:
//!
//! * [`verify_file`] hashes a cache hit before it can be served.
//! * [`tee_verified_to_cache`] streams an upstream response to the client
//!   immediately while hashing it in the background, promoting it into the
//!   cache only after the declared SRI matches.
//! * [`download_verified_to_temp`] hashes an upstream response into a
//!   temp file for mirror-less pass-through.
//! * [`stream_file`] yields an already verified file to the response.

use crate::{error::redact_url_credentials, storage::TarballWrite};
use axum::body::{Body, Bytes};
use futures_util::{Stream, StreamExt, stream};
use ssri::{Integrity, IntegrityChecker};
use std::{
    future::Future,
    io::{self, SeekFrom},
    path::PathBuf,
    pin::Pin,
};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt},
    sync::mpsc,
};

/// Chunk size for reading from a cached file. 64 KiB keeps syscall
/// overhead low without buffering a meaningful fraction of a
/// multi-MB tarball.
const READ_CHUNK: usize = 64 * 1024;

/// Backpressure budget for the upstream-tee channel. Each in-flight item
/// is one upstream chunk (a few kB), so 16 caps the writer's lead over the
/// client at ~1 MB. Once the buffer fills, the tee task awaits on `send`,
/// throttling the upstream read loop to the client's read rate.
const TEE_CHANNEL: usize = 16;

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

/// Hash a cached file against `integrity`, then rewind the same open
/// file so the caller can stream the exact bytes that were checked.
pub async fn verify_file(
    mut file: File,
    integrity: &Integrity,
) -> Result<File, TarballStreamError> {
    let mut checker = integrity_checker(integrity).map_err(TarballStreamError::Integrity)?;
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        let bytes_read = file.read(&mut buf).await.map_err(TarballStreamError::Io)?;
        if bytes_read == 0 {
            break;
        }
        checker.input(&buf[..bytes_read]);
    }
    checker.result().map_err(TarballStreamError::Integrity)?;
    file.seek(SeekFrom::Start(0)).await.map_err(TarballStreamError::Io)?;
    Ok(file)
}

/// Run once a teed body has been fully hashed and atomically promoted into
/// the proxy cache, with the verified byte length, to persist the cache
/// entry's integrity sidecar.
pub type CacheCommit = Box<dyn FnOnce(u64) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send>;

/// Stream an upstream response to the client immediately while teeing the
/// bytes into a temp file and hashing them in the background. The client
/// receives bytes as they arrive — the proxy does not buffer the whole body
/// first — and the cache copy is promoted (and `commit` run) only if the
/// complete body matches `integrity`, so the proxy cache never stores
/// unverified bytes. A hash mismatch, write error, upstream error, or client
/// disconnect abandons the temp file without promoting it; a body that exceeds
/// `max_bytes` additionally aborts the client stream, so the proxy never relays
/// unbounded attacker-controlled data.
///
/// Authoritative verification of the bytes a client *installs* is the
/// client's own SRI check against the packument it resolved; this proxy
/// guards only what it persists, trading emit-time verification for the
/// concurrency the streaming download regains.
pub fn tee_verified_to_cache(
    response: reqwest::Response,
    write: TarballWrite,
    integrity: Integrity,
    max_bytes: u64,
    commit: CacheCommit,
) -> Body {
    // The upstream URL is logged on several paths below; strip any embedded
    // credentials/secrets first, since the uplink URL is operator-provided.
    let url = redact_url_credentials(response.url().as_ref());
    let upstream = response.bytes_stream();
    let (tx, rx) = mpsc::channel::<Result<Bytes, io::Error>>(TEE_CHANNEL);
    tokio::spawn(run_tee_verified(
        Box::pin(upstream),
        write,
        tx,
        url,
        integrity,
        max_bytes,
        commit,
    ));
    let stream = stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|item| (item, rx)) });
    Body::from_stream(stream)
}

async fn run_tee_verified(
    mut upstream: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    write: TarballWrite,
    tx: mpsc::Sender<Result<Bytes, io::Error>>,
    url: String,
    integrity: Integrity,
    max_bytes: u64,
    commit: CacheCommit,
) {
    // `cache_write` drops to `None` the moment caching is abandoned (write
    // failure, oversized body, or a body that can't be hashed). The client
    // keeps receiving upstream bytes regardless — a failed cache write must
    // never truncate the response.
    let mut cache_write = Some(write);
    // `integrity` was already validated by `parse_integrity` upstream, so
    // this only fails defensively; abandon caching rather than promote an
    // unhashed body.
    let mut checker = match integrity_checker(&integrity) {
        Ok(checker) => Some(checker),
        Err(err) => {
            tracing::warn!(%url, %err, "integrity checker init failed; serving without cache");
            if let Some(write) = cache_write.take() {
                write.abandon().await;
            }
            None
        }
    };
    let mut streamed = 0u64;
    while let Some(chunk_result) = upstream.next().await {
        let chunk = match chunk_result {
            Ok(chunk) => chunk,
            Err(err) => {
                tracing::warn!(%url, %err, "upstream stream errored mid-body");
                let _ = tx.send(Err(io::Error::other(err.to_string()))).await;
                if let Some(write) = cache_write {
                    write.abandon().await;
                }
                return;
            }
        };
        // Cap the bytes relayed, not just the bytes cached: an oversized body is
        // aborted so pnpr never forwards unbounded attacker-controlled data.
        streamed = streamed.saturating_add(chunk.len() as u64);
        if streamed > max_bytes {
            tracing::warn!(%url, limit = max_bytes, received = streamed, "upstream body exceeds size cap; aborting");
            let _ = tx.send(Err(io::Error::other("tarball body exceeds size limit"))).await;
            if let Some(write) = cache_write {
                write.abandon().await;
            }
            return;
        }
        let mut abandon = false;
        if let Some(write) = cache_write.as_mut() {
            if let Err(err) = write.write_all(&chunk).await {
                tracing::warn!(%url, ?err, "cache temp-file write failed; continuing without cache");
                abandon = true;
            } else if let Some(checker) = checker.as_mut() {
                checker.input(&chunk);
            }
        }
        if abandon && let Some(write) = cache_write.take() {
            write.abandon().await;
        }
        if tx.send(Ok(chunk)).await.is_err() {
            // Client hung up. Stop the cache write too — a future request
            // will refetch and populate the cache cleanly.
            tracing::debug!(%url, "client disconnected mid-stream; abandoning cache write");
            if let Some(write) = cache_write {
                write.abandon().await;
            }
            return;
        }
    }
    let (Some(write), Some(checker)) = (cache_write, checker) else {
        return;
    };
    if let Err(err) = checker.result() {
        tracing::warn!(%url, %err, "upstream body failed integrity check; not caching");
        write.abandon().await;
        return;
    }
    if let Err(err) = write.finalize().await {
        tracing::warn!(%url, ?err, "cache finalize failed");
        return;
    }
    commit(streamed).await;
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
