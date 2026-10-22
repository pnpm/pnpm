use super::{
    Arc, BodyChunkSender, Duration, IgnoreEntryFilter, Instant, NetworkError, Reporter,
    STREAM_EXTRACT_COMPRESSED_THRESHOLD, STREAM_EXTRACT_DURING_DOWNLOAD_THRESHOLD, SemaphorePermit,
    StoreDir, Stream, StreamExt, TarballError, ThrottledClient, allocate_tarball_buffer,
    body_chunk_channel, fetch_error, redact_url_for_display, spawn_extraction,
};
use crate::{
    TarLimits,
    archive_integrity::ArchiveVerification,
    content_coding::ContentCoding,
    download::buffered::ExtractedFiles,
    extract::{BodyChunkReceiver, ExtractedResource, StreamedResource, stream_extract_channel},
};

/// How a body extracted while it downloads is decoded, checked, and stored.
pub(super) struct BodyVerification {
    pub(super) content_coding: ContentCoding,
    pub(super) verification: ArchiveVerification,
    pub(super) limits: TarLimits,
    pub(super) store_dir: &'static StoreDir,
    pub(super) ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
}

impl BodyVerification {
    /// Run the extractor on a blocking thread, reading the body from `chunks`.
    fn spawn(
        self,
        chunks: BodyChunkReceiver,
        permit: SemaphorePermit<'static>,
        package_url: &str,
    ) -> tokio::task::JoinHandle<ExtractedResource> {
        let package_url = package_url.to_owned();
        spawn_extraction(permit, move || {
            stream_extract_channel(
                chunks,
                self.content_coding,
                &StreamedResource {
                    verification: self.verification,
                    package_url: &package_url,
                    limits: self.limits,
                    store_dir: self.store_dir,
                    ignore_file_pattern: self.ignore_file_pattern.as_deref(),
                },
            )
        })
    }
}

/// Extract an archive body into the CAFS while it is still arriving.
///
/// The extractor runs on a blocking thread fed through a channel. It
/// removes the content coding, hashes the decoded resource, and extracts
/// it, so what is held in memory is the queued chunks plus the
/// extractor's own bounded batch — never the archive.
///
/// `seed` is the part of the body already pulled off the socket; the
/// caller has reported it to `progress`.
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
    verification: BodyVerification,
    progress: &mut BodyProgress<'_>,
    network_permit: Guard,
    streaming_permit: SemaphorePermit<'static>,
    http_client: &ThrottledClient,
    package_url: &str,
) -> Result<ExtractedFiles, TarballError>
where
    Reporter: self::Reporter,
    Body: Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
{
    let (chunk_tx, chunk_rx) = body_chunk_channel();
    let extract_task = verification.spawn(chunk_rx, streaming_permit, package_url);

    let mut feed = ExtractorFeed { chunk_tx, open: true };
    for chunk in seed {
        feed.send(chunk).await;
    }
    let body_error = pump_body::<Reporter, _>(&mut stream, progress, &mut feed, package_url).await;
    if let Some(error) = &body_error
        && error.is_fetch_timeout()
    {
        http_client.downscale_while_peers_active(package_url);
    }
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
    let result = rank_verdicts(body_error, extracted)?;
    tracing::info!(target: "pacquet::download", ?package_url, "Checksum verified");
    progress.finish::<Reporter>();
    Ok(result)
}

/// A network failure outranks an integrity mismatch, which outranks whatever
/// the bytes made the extractor do.
fn rank_verdicts(
    body_error: Option<TarballError>,
    resource: ExtractedResource,
) -> Result<ExtractedFiles, TarballError> {
    if let Some(error) = body_error {
        return Err(error);
    }
    let integrity = resource.integrity?;
    let (cas_paths, pkg_files_idx, bins) = resource.extracted?;
    Ok((integrity, cas_paths, pkg_files_idx, bins))
}

/// The extractor's end of the body stream. The extractor reads the whole
/// resource into its hash, so it closes the channel early only when it has
/// already reached a verdict; a closed channel stops the sends without being
/// an error of its own.
struct ExtractorFeed {
    chunk_tx: BodyChunkSender,
    open: bool,
}

impl ExtractorFeed {
    async fn send(&mut self, chunk: bytes::Bytes) {
        if self.open
            && self.chunk_tx
                .send(Ok(chunk))
                .await
                .is_err()
        {
            self.open = false;
        }
    }

    /// Tell the extractor the body ended early, so it fails instead of
    /// treating the truncated stream as a complete archive.
    async fn fail(&self) {
        if self.open {
            let _ = self.chunk_tx.send(Err(std::io::Error::other(
                "the tarball body failed mid-download",
            )))
            .await;
        }
    }
}

/// Feed the body to the extractor until it ends or the extractor stops
/// reading. Returns the network error that ended the body, if any.
async fn pump_body<Reporter, Body>(
    stream: &mut Body,
    progress: &mut BodyProgress<'_>,
    feed: &mut ExtractorFeed,
    package_url: &str,
) -> Option<TarballError>
where
    Reporter: self::Reporter,
    Body: Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
{
    while feed.open {
        match stream.next().await {
            Some(Ok(chunk)) => {
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
    None
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
    pub(super) expected_size: Option<u64>,
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
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| {
            inputs.http_client.downscale_on_timeout(inputs.package_url, &error);
            fetch_error(inputs.package_url, error)
        })?;
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
        // The buffer's capacity has doubled past what arrived; hand the
        // extractor the bytes, not the headroom.
        buf.shrink_to_fit();
        return Ok(Buffered::Overflowed(buf));
    }
    progress.warn_if_slow(inputs.http_client, inputs.package_url);
    progress.finish::<Reporter>();
    Ok(Buffered::Complete(buf))
}

/// Whether a body should be extracted while it downloads: its advertised
/// length says it is large, or, when it advertises none, the package's
/// unpacked size does.
pub(super) fn streams_while_downloading(
    expected_size: Option<u64>,
    unpacked_size: Option<usize>,
) -> bool {
    expected_size
        .or_else(|| unpacked_size.and_then(|size| u64::try_from(size).ok()))
        .is_some_and(|size| size >= STREAM_EXTRACT_DURING_DOWNLOAD_THRESHOLD)
}
