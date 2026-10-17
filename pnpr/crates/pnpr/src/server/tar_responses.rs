//! Encoding a tar registry's tarball response for the request.
//!
//! - `gzip`: the `.tgz` itself, which is the gzip encoding of its tar.
//! - `zstd`: the stored level-19 variant (see [`super::tar_variants`]), or
//!   until one exists the decoded tar, which the response compression layer
//!   encodes at its default level.
//! - neither: the decoded tar.

use super::{AppState, CanonicalPackageName, HeaderMap, Response, TarCopy, header, open_variant};
use axum::body::{Body, Bytes};
use futures_util::StreamExt;
use pnpr_storage::{Storage, streaming};
use ssri::{Algorithm, Integrity, IntegrityOpts};
use std::io::{self, Read};
use tokio::sync::mpsc;

/// Serve the tar of the `.tgz` response `tgz`, encoded as the request
/// accepts. `tgz_integrity` keys what is derived from it: the tar's integrity,
/// recorded once a decode finishes, and the level-19 variant.
pub(super) async fn tar_response(
    state: &AppState,
    headers: &HeaderMap,
    tgz: Response,
    name: CanonicalPackageName,
    tgz_integrity: Option<String>,
) -> Response {
    let storage = &state.inner.storage;
    let accepted = AcceptedCodings::of(headers);
    if accepted.zstd
        && let Some(key) = &tgz_integrity
    {
        if let Some((file, len)) = open_variant(storage, &name, key).await {
            return encoded_response(tgz, streaming::stream_file(file), Some(len), "zstd");
        }
        let copy = TarCopy::start(&state.inner.proxy.tar_variants, storage, &name, key).await;
        return decoded_response(storage.clone(), tgz, name, tgz_integrity, copy);
    }
    if accepted.gzip {
        let (parts, body) = tgz.into_parts();
        let len = parts.headers
            .get(header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok()?.parse().ok());
        return encoded_response(Response::from_parts(parts, Body::empty()), body, len, "gzip");
    }
    decoded_response(storage.clone(), tgz, name, tgz_integrity, None)
}

/// Which of the codings pnpr encodes tars in the request's `Accept-Encoding`
/// allows. A coding with `q=0` is refused, and `*` stands for any.
#[derive(Default)]
struct AcceptedCodings {
    zstd: bool,
    gzip: bool,
}

impl AcceptedCodings {
    fn of(headers: &HeaderMap) -> Self {
        headers
            .get_all(header::ACCEPT_ENCODING)
            .iter()
            .flat_map(|value| {
                value
                    .to_str()
                    .unwrap_or_default()
                    .split(',')
            })
            .filter_map(accepted_coding)
            .fold(Self::default(), |accepted, coding| match coding.as_str() {
                "zstd" => Self { zstd: true, ..accepted },
                "gzip" | "x-gzip" => Self { gzip: true, ..accepted },
                "*" => Self { zstd: true, gzip: true },
                _ => accepted,
            })
    }
}

/// The lowercased coding of one `Accept-Encoding` entry, or `None` when the
/// entry refuses it with `q=0`.
fn accepted_coding(entry: &str) -> Option<String> {
    let mut parts = entry.split(';');
    let coding = parts
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let refused = parts.any(|param| {
        param
            .trim()
            .strip_prefix("q=")
            .is_some_and(|quality| quality.trim().parse::<f32>() == Ok(0.0))
    });
    (!refused).then_some(coding)
}

/// `tgz`'s status and caller-scoping headers with an already encoded tar body.
fn encoded_response(tgz: Response, body: Body, len: Option<u64>, coding: &'static str) -> Response {
    let (mut parts, _) = tgz.into_parts();
    describe_tar_headers(&mut parts.headers);
    parts.headers.insert(header::CONTENT_ENCODING, header::HeaderValue::from_static(coding));
    if let Some(len) = len {
        parts.headers.insert(header::CONTENT_LENGTH, len.into());
    }
    Response::from_parts(parts, body)
}

/// Serve the tar `tgz` decodes to, recording its integrity once the whole of
/// it decoded and handing `copy` to the variant worker.
fn decoded_response(
    storage: Storage,
    tgz: Response,
    name: CanonicalPackageName,
    tgz_integrity: Option<String>,
    copy: Option<TarCopy>,
) -> Response {
    let (mut parts, body) = tgz.into_parts();
    describe_tar_headers(&mut parts.headers);
    let runtime = tokio::runtime::Handle::current();
    let body = gunzip_body(body, copy, move |tar_integrity| {
        let Some(key) = tgz_integrity else { return };
        runtime.spawn(async move {
            if let Err(err) =
                storage.record_tar_integrity(&name, &key, &tar_integrity.to_string()).await
            {
                tracing::warn!(?err, package = %name.as_str(), "recording a tar integrity failed");
            }
        });
    });
    Response::from_parts(parts, body)
}

/// Replace the `.tgz` response's representation headers with a tar's.
fn describe_tar_headers(headers: &mut HeaderMap) {
    for stale in [header::CONTENT_LENGTH, header::ETAG, header::ACCEPT_RANGES] {
        headers.remove(stale);
    }
    headers.remove("content-digest");
    headers.insert(header::CONTENT_TYPE, header::HeaderValue::from_static("application/x-tar"));
    headers.append(header::VARY, header::HeaderValue::from_static("accept-encoding"));
}

/// Compute the integrity of the tar a stored `.tgz` decodes to. `None` when
/// the `.tgz` is not a sequence of gzip members.
pub(super) fn tar_integrity_of(tgz: impl Read) -> Option<Integrity> {
    let mut decoder = flate2::read::MultiGzDecoder::new(tgz);
    let mut hasher = IntegrityOpts::new().algorithm(Algorithm::Sha512);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match decoder.read(&mut buf) {
            Ok(0) => return Some(hasher.result()),
            Ok(len) => hasher.input(&buf[..len]),
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
}

/// Decode a gzip body as it streams, calling `on_complete` with the
/// integrity of the decoded bytes once every member decoded, and copying them
/// into `copy`. A body that fails to decode ends the response with an error.
fn gunzip_body(
    body: Body,
    copy: Option<TarCopy>,
    on_complete: impl FnOnce(Integrity) + Send + 'static,
) -> Body {
    const CHANNEL_CHUNKS: usize = 16;
    let (compressed_tx, compressed_rx) = mpsc::channel::<io::Result<Bytes>>(CHANNEL_CHUNKS);
    let (tar_tx, tar_rx) = mpsc::channel::<io::Result<Bytes>>(CHANNEL_CHUNKS);
    tokio::spawn(forward_body(body, compressed_tx));
    tokio::task::spawn_blocking(move || {
        let decoder = flate2::read::MultiGzDecoder::new(ChannelReader::new(compressed_rx));
        let mut copy = copy;
        let integrity = decode_into(decoder, &tar_tx, &mut copy);
        if let Some(copy) = copy {
            if integrity.is_some() { copy.submit() } else { copy.discard() }
        }
        if let Some(integrity) = integrity {
            on_complete(integrity);
        }
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
    loop {
        let mut buf = vec![0u8; 64 * 1024];
        let len = match decoder.read(&mut buf) {
            Ok(0) => return Some(hasher.result()),
            Ok(len) => len,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => {
                let _ = out.blocking_send(Err(err));
                return None;
            }
        };
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
