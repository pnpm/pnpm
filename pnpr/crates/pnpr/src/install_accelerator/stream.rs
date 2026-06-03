//! Streamed install response: overlap the server's upstream tarball
//! fetch with sending files to the client, instead of fetching every
//! tarball into the store before replying.
//!
//! The response body is a gzip stream of length-prefixed frames, each
//! `[u8 tag][u32 BE payload_len][payload]`:
//!
//! * `L` — the resolved lockfile (`{ "lockfile": ... }`), sent first, as
//!   soon as resolution finishes.
//! * `I` — a store-index entry: `[u32 key_len][key][raw msgpack]`.
//! * `F` — a file: `[64-byte digest][1-byte exec][content]`.
//! * `S` — the final stats object.
//! * `E` — a mid-stream error (`{ "error": ... }`); ends the stream.
//!
//! A producer task resolves each client-missing package — reading the
//! file index straight from the download for uncached packages, or from
//! a store snapshot for ones the server already has — and streams its
//! `I` + `F` frames the moment that package is ready. Downloads run
//! concurrently while completed packages are compressed and sent, so the
//! client receives early files while later tarballs are still in flight.
//! See [pnpm/pnpm#12165](https://github.com/pnpm/pnpm/issues/12165).

use std::{
    collections::{HashMap, HashSet},
    io::Write as _,
    sync::Arc,
};

use axum::{
    body::{Body, Bytes},
    http::{StatusCode, header},
    response::Response,
};
use flate2::{Compression, write::GzEncoder};
use futures_util::stream::{FuturesUnordered, StreamExt as _, unfold};
use pacquet_config::Config as PacquetConfig;
use pacquet_lockfile::Lockfile;
use pacquet_network::ThrottledClient;
use pacquet_reporter::SilentReporter;
use pacquet_store_dir::{
    PackageFilesIndex, SharedVerifiedFilesCache, StoreIndex, StoreIndexWriter,
    encode_package_files_index, store_index_key,
};
use pacquet_tarball::{DownloadTarballToStore, RetryOpts};
use tokio::sync::mpsc;

use super::{FILES_GZIP_LEVEL, diff, hex_to_bytes, resolve::ResolvedPkg, stats_json};

/// Everything the producer task needs, all owned or `'static` so it can
/// outlive the request on a spawned task.
pub(super) struct StreamContext {
    pub config: &'static PacquetConfig,
    pub client: Arc<ThrottledClient>,
    pub lockfile: Lockfile,
    pub packages: Vec<ResolvedPkg>,
    pub store_integrities: Vec<String>,
    pub lockfile_only: bool,
}

/// Build the streamed install response: spawn the producer and hand axum
/// a body that drains its channel.
pub(super) fn streaming_response(ctx: StreamContext) -> Response {
    // Bounded so a slow client applies backpressure on the producer
    // (and, transitively, on how far ahead the downloads run) rather
    // than letting completed packages pile up unsent in memory.
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(8);
    tokio::spawn(produce(ctx, tx));

    let body =
        Body::from_stream(unfold(
            rx,
            |mut rx| async move { rx.recv().await.map(|item| (item, rx)) },
        ));

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, super::INLINE_CONTENT_TYPE)
        .header(header::CONTENT_ENCODING, "gzip")
        .body(body)
        .expect("streamed response is always valid")
}

/// Build a complete (non-streamed) body carrying a single `E` frame —
/// used for an input-lockfile verification failure, which is known before
/// any file would be streamed. `payload` is the frame's JSON
/// (`{ "violations": [...] }` or `{ "error": ... }`); the client's frame
/// parser handles it identically to a mid-stream `E`.
pub(super) fn error_response(payload: &serde_json::Value) -> Response {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::new(FILES_GZIP_LEVEL));
    write_frame(&mut encoder, b'E', payload.to_string().as_bytes());
    let body = encoder.finish().unwrap_or_default();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, super::INLINE_CONTENT_TYPE)
        .header(header::CONTENT_ENCODING, "gzip")
        .body(Body::from(body))
        .expect("error frame response is always valid")
}

/// A package's frames, ready to serialize: its store-index entry plus the
/// file contents the server materialized for it.
struct PackageOut {
    key: String,
    raw: Vec<u8>,
    files: Vec<FileOut>,
}

struct FileOut {
    digest: String,
    executable: bool,
    size: u64,
    content: Vec<u8>,
}

async fn produce(ctx: StreamContext, tx: mpsc::Sender<Result<Bytes, std::io::Error>>) {
    let StreamContext { config, client, lockfile, packages, store_integrities, lockfile_only } =
        ctx;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::new(FILES_GZIP_LEVEL));

    // `L` — the lockfile, available the moment resolution finished.
    let lockfile_json = serde_json::json!({ "lockfile": lockfile }).to_string();
    write_frame(&mut encoder, b'L', lockfile_json.as_bytes());
    if !flush_send(&mut encoder, &tx).await {
        return;
    }

    let mut stats = diff::Stats { total_packages: packages.len() as u64, ..diff::Stats::default() };

    // `--lockfile-only`: resolve and return the lockfile, fetch nothing.
    if lockfile_only {
        finish(encoder, &tx, &stats).await;
        return;
    }

    // Snapshot the server's store once: it gives the set of packages the
    // server already holds (read their file index without a download) and
    // the file digests the client already has (so we never re-send them).
    let snapshot = StoreIndex::open_readonly_in(&config.store_dir).ok();
    let present: HashSet<String> = snapshot
        .as_ref()
        .and_then(|index| index.keys().ok())
        .map(|keys| keys.into_iter().collect())
        .unwrap_or_default();
    let client_integrities: HashSet<&str> = store_integrities.iter().map(String::as_str).collect();
    let mut sent_digests = client_file_digests(snapshot.as_ref(), &client_integrities);

    // The writer persists freshly fetched index rows so a later request
    // finds them warm; reads of already-present rows come from `snapshot`.
    let (writer, writer_task) = StoreIndexWriter::spawn(&config.store_dir);

    let inflight = FuturesUnordered::new();
    for pkg in &packages {
        if client_integrities.contains(pkg.integrity.as_str()) {
            stats.already_in_store += 1;
            continue;
        }
        let key = store_index_key(&pkg.integrity, &pkg.pkg_id);
        let cached_index = if present.contains(&key) {
            snapshot.as_ref().and_then(|index| index.get(&key).ok().flatten())
        } else {
            None
        };
        inflight.push(build_package(
            pkg.pkg_id.clone(),
            pkg.integrity.clone(),
            pkg.tarball_url.clone(),
            cached_index,
            config,
            Arc::clone(&client),
            Arc::clone(&writer),
        ));
    }
    drop(snapshot);

    let mut inflight = inflight;
    let mut error: Option<String> = None;
    while let Some(result) = inflight.next().await {
        let pkg = match result {
            Ok(pkg) => pkg,
            Err(err) => {
                error = Some(err);
                break;
            }
        };
        stats.packages_to_fetch += 1;
        write_index_frame(&mut encoder, &pkg.key, &pkg.raw);
        for file in &pkg.files {
            stats.files_in_new_packages += 1;
            if sent_digests.insert((file.digest.clone(), file.executable)) {
                stats.files_to_download += 1;
                stats.download_bytes += file.size;
                if !write_file_frame(&mut encoder, file) {
                    error = Some(format!("invalid digest: {}", file.digest));
                    break;
                }
            } else {
                stats.files_already_in_cafs += 1;
            }
        }
        if error.is_some() || !flush_send(&mut encoder, &tx).await {
            break;
        }
    }

    drop(writer);
    let _ = writer_task.await;

    if let Some(message) = error {
        let payload = serde_json::json!({ "error": message }).to_string();
        write_frame(&mut encoder, b'E', payload.as_bytes());
        flush_send(&mut encoder, &tx).await;
        // Send whatever the trailer holds; the client already aborts on `E`.
        if let Ok(tail) = encoder.finish() {
            let _ = tx.send(Ok(Bytes::from(tail))).await;
        }
        return;
    }

    finish(encoder, &tx, &stats).await;
}

/// Resolve one client-missing package to its frames. Uncached packages
/// are downloaded (the call hands back the file index it just built);
/// already-present ones reuse `cached_index`. Either way the file
/// contents are read from the CAFS here, concurrently with other
/// packages' downloads.
async fn build_package(
    pkg_id: String,
    integrity: String,
    tarball_url: String,
    cached_index: Option<PackageFilesIndex>,
    config: &'static PacquetConfig,
    client: Arc<ThrottledClient>,
    writer: Arc<StoreIndexWriter>,
) -> Result<PackageOut, String> {
    let index = match cached_index {
        Some(index) => index,
        None => download_index(&pkg_id, &integrity, &tarball_url, config, &client, writer).await?,
    };

    let key = store_index_key(&integrity, &pkg_id);
    let raw = encode_package_files_index(&index).map_err(|err| err.to_string())?;

    let mut files = Vec::with_capacity(index.files.len());
    for info in index.files.values() {
        let executable = info.mode & 0o111 != 0;
        let mode = if executable { 0o755 } else { 0o644 };
        let path = config
            .store_dir
            .cas_file_path_by_mode(&info.digest, mode)
            .ok_or_else(|| format!("invalid digest: {}", info.digest))?;
        let content =
            tokio::fs::read(&path).await.map_err(|err| format!("{}: {err}", info.digest))?;
        files.push(FileOut { digest: info.digest.clone(), executable, size: info.size, content });
    }

    Ok(PackageOut { key, raw, files })
}

/// Fetch a package's tarball into the store and return the file index the
/// download just computed. A cache race (the row appeared after the
/// snapshot) yields no index from the download, so fall back to reading
/// the now-present row.
async fn download_index(
    pkg_id: &str,
    integrity: &str,
    tarball_url: &str,
    config: &'static PacquetConfig,
    client: &Arc<ThrottledClient>,
    writer: Arc<StoreIndexWriter>,
) -> Result<PackageFilesIndex, String> {
    if tarball_url.is_empty() {
        return Err(format!("no tarball url for {pkg_id}"));
    }
    let parsed = integrity.parse::<ssri::Integrity>().map_err(|err| err.to_string())?;
    let shared_index = StoreIndex::shared_readonly_in(&config.store_dir);
    let (_cas_paths, index) = DownloadTarballToStore {
        http_client: client,
        store_dir: &config.store_dir,
        store_index: shared_index,
        store_index_writer: Some(writer),
        verify_store_integrity: config.verify_store_integrity,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        package_integrity: &parsed,
        package_unpacked_size: None,
        package_url: tarball_url,
        package_id: pkg_id,
        auth_headers: &config.auth_headers,
        requester: "pnpr",
        prefetched_cas_paths: None,
        retry_opts: RetryOpts::default(),
        ignore_file_pattern: None,
        offline: false,
    }
    .run_without_mem_cache_with_index::<SilentReporter>()
    .await
    .map_err(|err| err.to_string())?;

    if let Some(index) = index {
        return Ok(index);
    }
    StoreIndex::open_readonly_in(&config.store_dir)
        .ok()
        .and_then(|store| store.get(&store_index_key(integrity, pkg_id)).ok().flatten())
        .ok_or_else(|| format!("no file index for {pkg_id}"))
}

/// The file digests (and their exec bit) the client already holds,
/// derived by reading the client's integrities out of the server
/// snapshot. Mirrors the diff pass's `client_digests` seed.
fn client_file_digests(
    snapshot: Option<&StoreIndex>,
    client_integrities: &HashSet<&str>,
) -> HashSet<(String, bool)> {
    let mut digests = HashSet::new();
    let Some(store) = snapshot else { return digests };
    let Ok(keys) = store.keys() else { return digests };
    let mut seen: HashSet<&str> = HashSet::new();
    let mut by_integrity: HashMap<&str, &str> = HashMap::new();
    for key in &keys {
        let Some((integrity, _pkg_id)) = key.split_once('\t') else { continue };
        if client_integrities.contains(integrity) && seen.insert(integrity) {
            by_integrity.insert(integrity, key.as_str());
        }
    }
    for key in by_integrity.values() {
        if let Ok(Some(index)) = store.get(key) {
            for file in index.files.values() {
                digests.insert((file.digest.clone(), file.mode & 0o111 != 0));
            }
        }
    }
    digests
}

fn write_frame(encoder: &mut GzEncoder<Vec<u8>>, tag: u8, payload: &[u8]) {
    let len = u32::try_from(payload.len()).unwrap_or(u32::MAX);
    let _ = encoder.write_all(&[tag]);
    let _ = encoder.write_all(&len.to_be_bytes());
    let _ = encoder.write_all(payload);
}

fn write_index_frame(encoder: &mut GzEncoder<Vec<u8>>, key: &str, raw: &[u8]) {
    let key_len = u32::try_from(key.len()).unwrap_or(u32::MAX);
    let mut payload = Vec::with_capacity(4 + key.len() + raw.len());
    payload.extend_from_slice(&key_len.to_be_bytes());
    payload.extend_from_slice(key.as_bytes());
    payload.extend_from_slice(raw);
    write_frame(encoder, b'I', &payload);
}

/// Write an `F` frame: `[64-byte digest][1-byte exec][content]`. Returns
/// `false` on a malformed digest (so the caller aborts with an error
/// frame rather than emitting a corrupt one).
fn write_file_frame(encoder: &mut GzEncoder<Vec<u8>>, file: &FileOut) -> bool {
    let Some(digest_bytes) = hex_to_bytes(&file.digest) else {
        return false;
    };
    let Ok(len) = u32::try_from(65 + file.content.len()) else {
        return false;
    };
    let _ = encoder.write_all(b"F");
    let _ = encoder.write_all(&len.to_be_bytes());
    let _ = encoder.write_all(&digest_bytes);
    let _ = encoder.write_all(&[u8::from(file.executable)]);
    let _ = encoder.write_all(&file.content);
    true
}

/// Flush the encoder's buffered output and send whatever it produced.
/// Returns `false` when the client has gone away (or gzip failed), so the
/// producer can stop early.
async fn flush_send(
    encoder: &mut GzEncoder<Vec<u8>>,
    tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
) -> bool {
    if encoder.flush().is_err() {
        let _ = tx.send(Err(std::io::Error::other("gzip flush failed"))).await;
        return false;
    }
    let chunk = std::mem::take(encoder.get_mut());
    if chunk.is_empty() {
        return true;
    }
    tx.send(Ok(Bytes::from(chunk))).await.is_ok()
}

/// Emit the final `S` stats frame and the gzip trailer.
async fn finish(
    mut encoder: GzEncoder<Vec<u8>>,
    tx: &mpsc::Sender<Result<Bytes, std::io::Error>>,
    stats: &diff::Stats,
) {
    write_frame(&mut encoder, b'S', stats_json(stats).to_string().as_bytes());
    if let Ok(tail) = encoder.finish() {
        let _ = tx.send(Ok(Bytes::from(tail))).await;
    }
}
