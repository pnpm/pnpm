//! Streaming helpers for the tarball path.
//!
//! Two flows live here:
//!
//! * [`stream_file`] — cache hit. Open the cached tarball and yield
//!   chunks straight to the response body.
//! * [`tee_to_cache`] — cache miss. Pull chunks from the upstream
//!   response, forward each chunk to the client *and* a temp file via
//!   an mpsc channel, then atomically promote the temp file to the
//!   final cache path on stream completion. On upstream error or
//!   client disconnect the temp file is removed.

use std::io;
use std::pin::Pin;

use axum::body::{Body, Bytes};
use futures_util::Stream;
use futures_util::stream::{self, StreamExt};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::cache::TarballWrite;

/// Chunk size for reading from a cached file. 64 KiB keeps syscall
/// overhead low without buffering a meaningful fraction of a
/// multi-MB tarball; matches the channel-element shape used for the
/// upstream tee path.
const READ_CHUNK: usize = 64 * 1024;

/// Backpressure budget for the upstream-tee channel. Each in-flight
/// item is one chunk from the upstream response (typically
/// hyper-sized — a few kB), so 16 caps the writer's lead over the
/// client at ~1 MB. Once the buffer fills, the tee task awaits
/// on `send`, which naturally throttles the upstream read loop to
/// the client's read rate.
const TEE_CHANNEL: usize = 16;

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

/// Stream an upstream [`reqwest::Response`] to the client while
/// teeing the bytes into a temp file owned by `write`. On clean
/// completion the temp file is `finalize`d (synced + renamed); on
/// upstream error or client disconnect it's abandoned.
pub fn tee_to_cache(response: reqwest::Response, write: TarballWrite) -> Body {
    let url = response.url().to_string();
    let upstream = response.bytes_stream();
    let (tx, rx) = mpsc::channel::<Result<Bytes, io::Error>>(TEE_CHANNEL);

    tokio::spawn(run_tee(Box::pin(upstream), write, tx, url));

    let stream = stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|item| (item, rx)) });
    Body::from_stream(stream)
}

async fn run_tee(
    mut upstream: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    write: TarballWrite,
    tx: mpsc::Sender<Result<Bytes, io::Error>>,
    url: String,
) {
    // `cache_write` goes to `None` after a write failure: the temp
    // file is abandoned and the client continues to receive bytes
    // from upstream. The cache is best-effort — matching the
    // fallback that `serve_tarball` already does when `open_tarball_tmp`
    // fails (see `streaming without cache` log).
    let mut cache_write = Some(write);
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
        if let Some(write) = cache_write.as_mut()
            && let Err(err) = write.file.write_all(&chunk).await
        {
            tracing::warn!(%url, ?err, "cache temp-file write failed; continuing without cache");
            if let Some(write) = cache_write.take() {
                write.abandon().await;
            }
        }
        if tx.send(Ok(chunk)).await.is_err() {
            // Client hung up. Don't keep streaming bytes nobody's
            // reading; abandon the partial cache file too — a future
            // request will refetch and (if it completes) populate
            // the cache cleanly. Salvaging the partial write is
            // possible (keep going, finalize at end) but adds the
            // failure mode where a client that aborted *also* poisoned
            // a half-written upstream into our cache.
            tracing::debug!(%url, "client disconnected mid-stream; abandoning cache write");
            if let Some(write) = cache_write {
                write.abandon().await;
            }
            return;
        }
    }
    if let Some(write) = cache_write
        && let Err(err) = write.finalize().await
    {
        tracing::warn!(%url, ?err, "cache finalize failed");
    }
}
