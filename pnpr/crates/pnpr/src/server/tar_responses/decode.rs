use super::{CheckedReader, StaleTgz, TarCopy};
use axum::body::{Body, Bytes};
use futures_util::StreamExt;
use ssri::{Algorithm, Integrity, IntegrityChecker, IntegrityOpts};
use std::io::{self, Read};
use tokio::sync::mpsc;

/// The most a `.tgz` may decode to before pnpr stops: what pnpm's default
/// decoded-output budget admits, 4 GiB of files plus 256 MiB of headers.
/// Above it the version has no tar: publish records no tar hash, and a tar
/// response ends with an error.
const MAX_DECODED_TAR_BYTES: u64 = (4 * 1024 + 256) * 1024 * 1024;

/// Compute the integrity of the tar a stored `.tgz` decodes to. `None` when
/// the `.tgz` is not a sequence of gzip members.
pub(in super::super) fn tar_integrity_of(tgz: impl Read) -> Option<Integrity> {
    tar_integrity_within(tgz, MAX_DECODED_TAR_BYTES)
}

pub(super) fn tar_integrity_within(tgz: impl Read, limit: u64) -> Option<Integrity> {
    let mut decoder = flate2::read::MultiGzDecoder::new(tgz).take(limit.saturating_add(1));
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
    /// Whether the bytes are known to be the version's, so the tar's
    /// integrity and variant may be derived from them.
    pub(super) derive: bool,
    pub(super) copy: Option<TarCopy>,
    pub(super) stale: StaleTgz,
}

/// Decode a gzip body as it streams, copying the decoded bytes into
/// `derived.copy`. Once every member decoded and the compressed bytes matched
/// `derived.expected`, submit the copy and call `on_complete` with the
/// integrity of the decoded bytes. A body that fails to decode ends the
/// response with an error.
pub(super) fn gunzip_body(
    body: Body,
    derived: DerivedFrom,
    on_complete: impl FnOnce(Integrity) + Send + 'static,
) -> Body {
    const CHANNEL_CHUNKS: usize = 16;
    let (compressed_tx, compressed_rx) = mpsc::channel::<io::Result<Bytes>>(CHANNEL_CHUNKS);
    let (tar_tx, tar_rx) = mpsc::channel::<io::Result<Bytes>>(CHANNEL_CHUNKS);
    tokio::spawn(forward_body(body, compressed_tx));
    tokio::task::spawn_blocking(move || {
        derive_from_decode(compressed_rx, &tar_tx, derived, on_complete);
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
    let DerivedFrom { check, derive, mut copy, stale } = derived;
    let mut compressed = CheckedReader {
        inner: ChannelReader::new(compressed_rx),
        checker: check.map(IntegrityChecker::new),
    };
    let decoder = flate2::read::MultiGzDecoder::new(&mut compressed)
        .take(MAX_DECODED_TAR_BYTES.saturating_add(1));
    let Some(integrity) = decode_into(decoder, tar_tx, &mut copy) else { return };
    if compressed.checker.is_some_and(|checker| checker.result().is_err()) {
        let _ = tar_tx.blocking_send(Err(stale.evict()));
        return;
    }
    if !derive {
        return;
    }
    if let Some(copy) = copy {
        copy.submit();
    }
    on_complete(integrity);
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

/// Send what `decoder` reads to `out` and into `copy`, returning the integrity
/// of all of it once it ends. `None` when the decode failed or the response
/// was dropped.
fn decode_into(
    mut decoder: impl Read,
    out: &mpsc::Sender<io::Result<Bytes>>,
    copy: &mut Option<TarCopy>,
) -> Option<Integrity> {
    let mut hasher = IntegrityOpts::new().algorithm(Algorithm::Sha512);
    let mut decoded = 0u64;
    loop {
        let mut buf = vec![0u8; 64 * 1024];
        let len = match decoder.read(&mut buf) {
            Ok(0) if decoded > MAX_DECODED_TAR_BYTES => {
                let _ = out.blocking_send(Err(io::Error::other(format!(
                    "the tar exceeds the {MAX_DECODED_TAR_BYTES}-byte decode limit",
                ))));
                return None;
            }
            Ok(0) => return Some(hasher.result()),
            Ok(len) => len,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => {
                let _ = out.blocking_send(Err(err));
                return None;
            }
        };
        decoded += len as u64;
        buf.truncate(len);
        hasher.input(&buf);
        *copy = copy
            .take()
            .and_then(|copy| copy.write(&buf));
        out.blocking_send(Ok(Bytes::from(buf)))
            .ok()?;
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
