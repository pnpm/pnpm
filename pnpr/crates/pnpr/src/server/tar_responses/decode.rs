use super::{CheckedReader, StaleTgz, TarCopy};
use axum::body::{Body, Bytes};
use futures_util::StreamExt;
use ssri::{Algorithm, Integrity, IntegrityChecker, IntegrityOpts};
use std::{
    io::{self, Read},
    sync::Arc,
};
use tokio::sync::{Semaphore, mpsc};

/// The most a `.tgz` may decode to before pnpr stops: what pnpm's default
/// decoded-output budget admits, 4 GiB of files plus 256 MiB of headers.
/// Above it the version has no tar: publish records no tar hash, and a tar
/// response ends with an error.
const MAX_DECODED_TAR_BYTES: u64 = (4 * 1024 + 256) * 1024 * 1024;

/// Tar responses and publishes that may decode at once. Each holds a
/// blocking thread for as long as its client reads or its tar hashes, so the
/// cap keeps slow or many clients from taking the whole blocking pool. A
/// response waits for a permit, and stops waiting when its client goes away.
pub(in super::super) const MAX_CONCURRENT_TAR_DECODES: usize = 64;

/// What pnpm admits in a tar beyond the file payload `dist.unpackedSize`
/// declares: 256 MiB of headers and skipped entries, and 10 KiB of
/// end-of-archive zeros.
const NON_PAYLOAD_BYTES: u64 = 256 * 1024 * 1024 + 10 * 1024;

/// The most a tar response decodes for a version that declares
/// `unpacked_size`: past it, pnpm would reject the tar anyway.
pub(in super::super) fn tar_decode_limit(unpacked_size: Option<u64>) -> u64 {
    unpacked_size.map_or(MAX_DECODED_TAR_BYTES, |size| {
        size.saturating_add(NON_PAYLOAD_BYTES).min(MAX_DECODED_TAR_BYTES)
    })
}

/// Compute the integrity of the tar a stored `.tgz` decodes to. `None` when
/// the `.tgz` is not a sequence of gzip members.
pub(in super::super) fn tar_integrity_of(tgz: impl Read) -> Option<Integrity> {
    tar_integrity_within(tgz, MAX_DECODED_TAR_BYTES)
}

/// Compute the integrity of the tar a stored tar resource holds: the tar
/// itself, or with `gzip`, its gzip encoding. `None` when a gzip encoding does
/// not decode in full, or the tar is larger than any `.tgz` may decode to.
pub(in super::super) fn tar_resource_integrity(
    resource: impl Read,
    gzip: bool,
) -> Option<Integrity> {
    if gzip {
        return tar_integrity_of(resource);
    }
    hash_within(resource, MAX_DECODED_TAR_BYTES)
}

pub(super) fn tar_integrity_within(tgz: impl Read, limit: u64) -> Option<Integrity> {
    hash_within(flate2::read::MultiGzDecoder::new(tgz), limit)
}

fn hash_within(tar: impl Read, limit: u64) -> Option<Integrity> {
    let mut decoder = tar.take(limit.saturating_add(1));
    let mut decoded = 0u64;
    let mut hasher = IntegrityOpts::new().algorithm(Algorithm::Sha512);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match decoder.read(&mut buf) {
            Ok(0) => return (decoded <= limit).then(|| hasher.result()),
            Ok(len) => {
                decoded += len as u64;
                hasher.input(&buf[..len]);
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
}

/// What a decode may derive from the tar it decodes.
pub(super) struct DerivedFrom {
    /// The `.tgz` integrity the compressed bytes still have to match. A
    /// mismatch fails the response and evicts the stale copy.
    pub(super) check: Option<Integrity>,
    /// Whether to hash the tar for `on_complete`: the version has an
    /// integrity to key it by, and its tar's is not recorded yet.
    pub(super) hash: bool,
    pub(super) copy: Option<TarCopy>,
    pub(super) stale: StaleTgz,
    /// See [`tar_decode_limit`].
    pub(super) limit: u64,
}

/// Decode a gzip body as it streams, copying the decoded bytes into
/// `derived.copy`. Once every member decoded and the compressed bytes matched
/// `derived.check`, submit the copy and, if `derived.hash`, call
/// `on_complete` with the integrity of the decoded bytes. A body that fails to decode ends the
/// response with an error.
pub(super) fn gunzip_body(
    body: Body,
    derived: DerivedFrom,
    decodes: Arc<Semaphore>,
    on_complete: impl FnOnce(Integrity) + Send + 'static,
) -> Body {
    const CHANNEL_CHUNKS: usize = 16;
    let (compressed_tx, compressed_rx) = mpsc::channel::<io::Result<Bytes>>(CHANNEL_CHUNKS);
    let (tar_tx, tar_rx) = mpsc::channel::<io::Result<Bytes>>(CHANNEL_CHUNKS);
    tokio::spawn(forward_body(body, compressed_tx));
    tokio::spawn(async move {
        let permit = tokio::select! {
            permit = decodes.acquire_owned() => permit.expect("the decode semaphore is never closed"),
            () = tar_tx.closed() => return,
        };
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            derive_from_decode(compressed_rx, &tar_tx, derived, on_complete);
        });
    });
    let chunks = futures_util::stream::unfold(tar_rx, |mut tar_rx| async move {
        tar_rx
            .recv()
            .await
            .map(|chunk| (chunk, tar_rx))
    });
    // The compression layer polls its input once more after it ends.
    Body::from_stream(chunks.fuse())
}

/// Decode the compressed chunks into `tar_tx`, then derive from them what
/// `derived` allows once they matched its digest.
fn derive_from_decode(
    compressed_rx: mpsc::Receiver<io::Result<Bytes>>,
    tar_tx: &mpsc::Sender<io::Result<Bytes>>,
    derived: DerivedFrom,
    on_complete: impl FnOnce(Integrity),
) {
    let DerivedFrom { check, hash, mut copy, stale, limit } = derived;
    let mut compressed = CheckedReader {
        inner: ChannelReader::new(compressed_rx),
        checker: check.map(IntegrityChecker::new),
    };
    let decoder = flate2::read::MultiGzDecoder::new(&mut compressed).take(limit.saturating_add(1));
    let mut hasher = hash.then(|| IntegrityOpts::new().algorithm(Algorithm::Sha512));
    if !decode_into(decoder, tar_tx, &mut copy, hasher.as_mut(), limit) {
        return;
    }
    if compressed.checker.is_some_and(|checker| checker.result().is_err()) {
        let _ = tar_tx.blocking_send(Err(stale.evict()));
        return;
    }
    if let Some(copy) = copy {
        copy.submit();
    }
    if let Some(hasher) = hasher {
        on_complete(hasher.result());
    }
}

async fn forward_body(body: Body, chunks: mpsc::Sender<io::Result<Bytes>>) {
    let mut body = body.into_data_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(io::Error::other);
        let failed = chunk.is_err();
        if chunks.send(chunk).await.is_err() || failed {
            return;
        }
    }
}

/// Send what `decoder` reads to `out`, into `copy`, and into `hasher`.
/// `false` when the decode failed or the response was dropped.
fn decode_into(
    mut decoder: impl Read,
    out: &mpsc::Sender<io::Result<Bytes>>,
    copy: &mut Option<TarCopy>,
    mut hasher: Option<&mut IntegrityOpts>,
    limit: u64,
) -> bool {
    let mut decoded = 0u64;
    loop {
        let mut buf = vec![0u8; 64 * 1024];
        let len = match decoder.read(&mut buf) {
            Ok(0) if decoded > limit => {
                let _ = out.blocking_send(Err(io::Error::other(format!(
                    "the tar exceeds the {limit}-byte decode limit",
                ))));
                return false;
            }
            Ok(0) => return true,
            Ok(len) => len,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => {
                let _ = out.blocking_send(Err(err));
                return false;
            }
        };
        decoded += len as u64;
        buf.truncate(len);
        if let Some(hasher) = hasher.as_deref_mut() {
            hasher.input(&buf);
        }
        *copy = copy
            .take()
            .and_then(|copy| copy.write(&buf));
        if out
            .blocking_send(Ok(Bytes::from(buf)))
            .is_err()
        {
            return false;
        }
    }
}

/// Blocking [`Read`] over a channel of body chunks, for a decoder on a
/// blocking thread. A closed channel is end-of-stream.
struct ChannelReader {
    rx: mpsc::Receiver<io::Result<Bytes>>,
    current: Bytes,
}

impl ChannelReader {
    fn new(rx: mpsc::Receiver<io::Result<Bytes>>) -> Self {
        Self { rx, current: Bytes::new() }
    }
}

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        while self.current.is_empty() {
            match self.rx.blocking_recv() {
                Some(chunk) => self.current = chunk?,
                None => return Ok(0),
            }
        }
        let len = self.current.len().min(buf.len());
        buf[..len].copy_from_slice(&self.current.split_to(len));
        Ok(len)
    }
}
