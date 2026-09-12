use super::{
    Algorithm, Arc, BodyChunkSender, Duration, GZIP_MAGIC, HashMap, IgnoreEntryFilter, Instant,
    Integrity, IntegrityChecker, IntegrityOpts, NetworkError, PackageFilesIndex, PathBuf, Reporter,
    STREAM_EXTRACT_COMPRESSED_THRESHOLD, STREAM_EXTRACT_DURING_DOWNLOAD_THRESHOLD, SemaphorePermit,
    StoreDir, Stream, StreamExt, TarballError, ThrottledClient, VerifyChecksumError,
    allocate_tarball_buffer, body_chunk_channel, fetch_error, non_gzip_body_error,
    redact_url_for_display, spawn_extraction, stream_extract_gzipped_channel,
};

/// Hashes a tarball body chunk by chunk as it arrives: verifying the
/// pinned integrity when the resolution carries one, computing a fresh
/// sha512 when it does not.
///
/// Same two outcomes [`verify_tarball_integrity`](crate::download::verify_tarball_integrity) produces over a fully
/// buffered body. Doing it incrementally is what lets a body be
/// extracted while it downloads without the whole of it being held to
/// hash at the end.
pub(super) enum BodyHasher {
    Pinned { expected: Integrity, checker: IntegrityChecker },
    Computed(IntegrityOpts),
}

impl BodyHasher {
    pub(super) fn new(expected_integrity: Option<&Integrity>) -> Self {
        match expected_integrity {
            Some(expected) => BodyHasher::Pinned {
                expected: expected.clone(),
                checker: IntegrityChecker::new(expected.clone()),
            },
            None => BodyHasher::Computed(IntegrityOpts::new().algorithm(Algorithm::Sha512)),
        }
    }

    fn input(&mut self, bytes: &[u8]) {
        match self {
            BodyHasher::Pinned { checker, .. } => checker.input(bytes),
            BodyHasher::Computed(opts) => opts.input(bytes),
        }
    }

    fn finish(self, package_url: &str) -> Result<Integrity, TarballError> {
        match self {
            BodyHasher::Pinned { expected, checker } => {
                checker.result().map(|_| expected).map_err(|error| {
                    TarballError::Checksum(VerifyChecksumError {
                        url: package_url.to_string(),
                        error,
                    })
                })
            }
            BodyHasher::Computed(opts) => Ok(opts.result()),
        }
    }
}

/// Extract a gzipped tarball body into the CAFS while it is still
/// arriving, hashing every byte on its way past.
///
/// The extractor runs on a blocking thread fed through a channel, so
/// what is held in memory is the queued chunks plus the extractor's own
/// bounded batch — never the archive. That is the property the callers
/// want: [`fetch_and_extract_once`](crate::download::fetch::fetch_and_extract_once) takes this path up front for a body
/// whose advertised size already says it is large, and falls back to it
/// for one that turns out to be large while it buffers.
///
/// `seed` is the part of the body already pulled off the socket; the
/// caller has reported it to `progress` and has not hashed it.
/// `network_permit` is dropped once the body is done, before the wait
/// on the extractor's CPU work.
///
/// Entries reach the CAFS before the body is verified. Only returning
/// `Ok` makes them reachable, and a caller that gets an error treats
/// the fetch as failed, so a tampered body's entries stay orphaned in
/// the content-addressed store rather than being installed.
#[expect(
    clippy::too_many_arguments,
    reason = "the parameters are the independent pieces of one in-flight download; a struct would only move the same fields into a wrapper"
)]
pub(super) async fn extract_body_while_downloading<Reporter, Body, Guard>(
    seed: Vec<bytes::Bytes>,
    mut stream: Body,
    mut hasher: BodyHasher,
    progress: &mut BodyProgress<'_>,
    network_permit: Guard,
    streaming_permit: SemaphorePermit<'static>,
    http_client: &ThrottledClient,
    package_url: &str,
    store_dir: &'static StoreDir,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError>
where
    Reporter: self::Reporter,
    Body: Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
{
    let (chunk_tx, chunk_rx) = body_chunk_channel();
    let extractor_ignore = ignore_file_pattern.clone();
    let extract_task = spawn_extraction(streaming_permit, move || {
        stream_extract_gzipped_channel(chunk_rx, store_dir, extractor_ignore.as_deref())
    });

    let mut feed = ExtractorFeed { chunk_tx, open: true };
    for chunk in seed {
        hasher.input(&chunk);
        feed.send(chunk).await;
    }
    let body_error =
        pump_body::<Reporter, _>(&mut stream, &mut hasher, progress, &mut feed, package_url).await;
    if body_error.is_none() {
        progress.warn_if_slow(http_client, package_url);
    }

    // Close the channel so the extractor sees end-of-stream. Release
    // the network permit before waiting on CPU work because the body is
    // done or abandoned after a network error.
    drop(feed);
    drop(stream);
    drop(network_permit);
    tracing::info!(target: "pacquet::download", ?package_url, "Download completed");
    let extracted = extract_task.await.map_err(TarballError::TaskJoin)?;
    if let Some(error) = body_error {
        return Err(error);
    }
    let integrity = hasher.finish(package_url)?;
    let (cas_paths, pkg_files_idx) = extracted?;
    tracing::info!(target: "pacquet::download", ?package_url, "Checksum verified");
    progress.finish::<Reporter>();
    Ok((integrity, cas_paths, pkg_files_idx))
}

/// The extractor's end of the body stream. The extractor can legitimately
/// finish early — the tar terminator and gzip trailer arrive while chunks
/// are still in flight — so a closed channel stops the sends without being
/// an error verdict.
struct ExtractorFeed {
    chunk_tx: BodyChunkSender,
    open: bool,
}

impl ExtractorFeed {
    async fn send(&mut self, chunk: bytes::Bytes) {
        if self.open && self.chunk_tx.send(Ok(chunk)).await.is_err() {
            self.open = false;
        }
    }

    /// Tell the extractor the body ended early, so it fails instead of
    /// treating the truncated stream as a complete archive.
    async fn fail(&self) {
        if self.open {
            let _ = self
                .chunk_tx
                .send(Err(std::io::Error::other("the tarball body failed mid-download")))
                .await;
        }
    }
}

/// Hash the body to its end — the integrity verdict covers every byte —
/// while feeding it to the extractor. Returns the network error that ended
/// the body, if any.
async fn pump_body<Reporter, Body>(
    stream: &mut Body,
    hasher: &mut BodyHasher,
    progress: &mut BodyProgress<'_>,
    feed: &mut ExtractorFeed,
    package_url: &str,
) -> Option<TarballError>
where
    Reporter: self::Reporter,
    Body: Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
{
    loop {
        match stream.next().await {
            Some(Ok(chunk)) => {
                hasher.input(&chunk);
                progress.on_chunk::<Reporter>(chunk.len());
                feed.send(chunk).await;
            }
            Some(Err(error)) => {
                feed.fail().await;
                return Some(TarballError::FetchTarball(NetworkError::new(package_url, error)));
            }
            None => return None,
        }
    }
}

/// Emits download progress for both body paths of [`fetch_and_extract_once`](crate::download::fetch::fetch_and_extract_once).
///
/// Mirrors `lodash.throttle(opts.onProgress, 500)` on pnpm's side,
/// down to the leading and trailing edges. The size gate exists
/// because the default reporter renders a percent gauge: without a
/// `Content-Length` there is no denominator, and for a typical
/// sub-megabyte package the gauge would reach 100% before any UI tick
/// could show it.
pub(crate) struct BodyProgress<'a> {
    pub(super) emit: bool,
    pub(super) started_at: Instant,
    pub(super) last_emit: Option<Instant>,
    pub(super) last_emitted_downloaded: u64,
    pub(super) downloaded: u64,
    pub(super) package_id: &'a str,
}

pub(crate) fn slow_download_warning(
    downloaded: u64,
    elapsed: Duration,
    fetch_min_speed_ki_bps: u64,
    package_url: &str,
) -> Option<String> {
    let elapsed_ms = elapsed.as_millis();
    if downloaded == 0 || elapsed_ms <= 1_000 {
        return None;
    }
    let avg_ki_bps =
        u128::from(downloaded).saturating_mul(1_000) / elapsed_ms.saturating_mul(1_024);
    if avg_ki_bps >= u128::from(fetch_min_speed_ki_bps) {
        return None;
    }
    let size_ki_b = downloaded / 1_024;
    Some(format!(
        "Tarball download average speed {avg_ki_bps} KiB/s (size {size_ki_b} KiB) is below {fetch_min_speed_ki_bps} KiB/s: {} (GET)",
        redact_url_for_display(package_url),
    ))
}

/// The outcome of accumulating a response body in memory.
pub(super) enum Buffered {
    /// The whole body fits in memory.
    Complete(Vec<u8>),
    /// The body grew past the streaming threshold; the extractor takes the
    /// rest of it, starting from these bytes.
    Overflowed(Vec<u8>),
}

pub(super) struct BufferBody<'a, 'progress, Body> {
    pub(super) stream: &'a mut Body,
    pub(super) progress: &'a mut BodyProgress<'progress>,
    /// The bytes already pulled to decide the gzip magic.
    pub(super) prefix: Vec<bytes::Bytes>,
    pub(super) prefix_len: usize,
    pub(super) expected_size: Option<u64>,
    pub(super) expected_integrity: Option<&'a Integrity>,
    pub(super) is_gzip: bool,
    pub(super) package_url: &'a str,
    pub(super) http_client: &'a ThrottledClient,
}

pub(super) async fn buffer_body<Reporter, Body>(
    inputs: BufferBody<'_, '_, Body>,
) -> Result<Buffered, TarballError>
where
    Reporter: self::Reporter,
    Body: Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
{
    let BufferBody { stream, progress, .. } = inputs;

    // Pre-size from the advertised length, but only as far as this
    // path will ever fill: past the threshold below the body is
    // handed to the streaming extractor, so reserving for a larger
    // advertised size would be reserving for bytes that never land
    // here — and would let a server's claim, rather than its body,
    // decide the size of an allocation.
    let reserve =
        inputs.expected_size.map(|size| size.min(STREAM_EXTRACT_COMPRESSED_THRESHOLD as u64));
    let mut buf = allocate_tarball_buffer(reserve, inputs.package_url)?;
    for chunk in inputs.prefix {
        buf.extend_from_slice(&chunk);
        progress.on_chunk::<Reporter>(chunk.len());
    }
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| fetch_error(inputs.package_url, error))?;
        buf.extend_from_slice(&chunk);
        progress.on_chunk::<Reporter>(chunk.len());
        // Nothing above bounds how much body a server may send: a
        // chunked response advertises no length at all, and an
        // advertised one is a claim like any other. Once the body
        // has grown to the size at which it would be extracted as a
        // stream anyway, stop accumulating it.
        if buf.len() < STREAM_EXTRACT_COMPRESSED_THRESHOLD {
            continue;
        }
        if !inputs.is_gzip {
            return Err(drain_non_gzip_body::<Reporter, _>(
                stream,
                progress,
                buf,
                inputs.expected_integrity,
                inputs.package_url,
                inputs.prefix_len,
            )
            .await);
        }
        // The buffer's capacity has doubled past what arrived; hand the
        // extractor the bytes, not the headroom.
        buf.shrink_to_fit();
        return Ok(Buffered::Overflowed(buf));
    }
    progress.warn_if_slow(inputs.http_client, inputs.package_url);
    progress.finish::<Reporter>();
    Ok(Buffered::Complete(buf))
}

pub(super) fn starts_with_gzip_magic(prefix: &[bytes::Bytes]) -> bool {
    let mut magic = prefix.iter().flat_map(|chunk| chunk.iter().copied());
    (magic.next(), magic.next()) == (Some(GZIP_MAGIC[0]), Some(GZIP_MAGIC[1]))
}

pub(super) fn advertises_large_body(expected_size: Option<u64>) -> bool {
    expected_size.is_some_and(|size| size >= STREAM_EXTRACT_DURING_DOWNLOAD_THRESHOLD)
}

/// A body that does not start with the gzip magic fails at the decoder
/// however much of it arrives, so the rest is read and dropped rather than
/// kept. It still has to be read: when the resolution pins an integrity, a
/// body that does not hash to it is a tampered or stale download, and saying
/// so outranks saying it did not decode.
async fn drain_non_gzip_body<Reporter, Body>(
    stream: &mut Body,
    progress: &mut BodyProgress<'_>,
    buf: Vec<u8>,
    expected_integrity: Option<&Integrity>,
    package_url: &str,
    prefix_len: usize,
) -> TarballError
where
    Reporter: self::Reporter,
    Body: Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
{
    let mut hasher = BodyHasher::new(expected_integrity);
    hasher.input(&buf);
    drop(buf);
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) => {
                hasher.input(&chunk);
                progress.on_chunk::<Reporter>(chunk.len());
            }
            Err(error) => return fetch_error(package_url, error),
        }
    }
    match hasher.finish(package_url) {
        Ok(_) => non_gzip_body_error(prefix_len),
        Err(error) => error,
    }
}
