//! Downloading a tarball into the content-addressable store.
//!
//! Owns the retry policy, integrity verification, and the progress
//! events the reporter renders during a fetch.

use super::{
    Arc, Duration, GZIP_MAGIC, HashMap, HttpStatusError, IgnoreEntryFilter, Instant, NetworkError,
    PathBuf, PrefetchedCasPaths, STREAM_EXTRACT_COMPRESSED_THRESHOLD,
    STREAM_EXTRACT_DURING_DOWNLOAD_THRESHOLD, SharedReportedProgressKeys, TarballError,
    VerifyChecksumError, allocate_tarball_buffer, auth_header_for_package_download,
    body_chunk_channel, extract_gzipped_tarball, local_file_tarball_path, non_gzip_body_error,
    open_local_tarball, post_download_semaphore, read_local_tarball_buffer,
    stream_extract_gzipped_channel, streaming_extract_semaphore,
};
use futures_util::{Stream, StreamExt};
use pnpm_network::{
    AuthHeaders, MAX_THROUGHPUT_PRIORITY, RetryOpts, ThrottledClient, redact_url_for_display,
};
use pnpm_reporter::{
    FetchingProgressLog, FetchingProgressMessage, LogEvent, LogLevel, ProgressLog, ProgressMessage,
    Reporter, RequestRetryError, RequestRetryLog,
};
use pnpm_store_dir::{
    PackageFilesIndex, SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreDir,
    StoreIndexWriter, store_index_key,
};
use ssri::{Algorithm, Integrity, IntegrityChecker, IntegrityOpts};

/// This subroutine downloads and extracts a tarball to the store directory.
///
/// It returns a CAS map of files in the tarball.
///
/// `Clone` is cheap — every field is a reference, a `Copy` scalar, or an
/// `Arc` — so a caller can keep a copy to retry through a different entry
/// point (e.g. fall back to [`Self::run_without_mem_cache`] after a
/// best-effort [`Self::run_with_mem_cache`] reports a sibling failure).
#[derive(Clone)]
#[must_use]
pub struct DownloadTarballToStore<'a> {
    pub http_client: &'a ThrottledClient,
    pub store_dir: &'static StoreDir,
    /// Shared read-only handle to the `SQLite` store index. `None` when the
    /// store does not (yet) have an `index.db`, in which case every cache
    /// lookup short-circuits to a network fetch. Callers open this once per
    /// install and pass the same handle to every [`DownloadTarballToStore`]
    /// so we don't reopen the DB per package.
    pub store_index: Option<SharedReadonlyStoreIndex>,
    /// Handle to the batched store-index writer. Each successful tarball
    /// extraction queues one `(key, PackageFilesIndex)` row; a single
    /// writer task drains the channel and flushes batches of up to 256 in
    /// one transaction each, so the whole install goes through one
    /// `Connection::open` and a handful of WAL commits. Opening a
    /// connection per tarball instead would saturate tokio's blocking
    /// pool — 500+ threads on a 1352-snapshot install, see [#263].
    /// `None` degrades to "skip index row", matching the read
    /// side's stance: install still succeeds, the next install misses on
    /// this cache key and re-downloads.
    ///
    /// [#263]: https://github.com/pnpm/pacquet/issues/263
    pub store_index_writer: Option<Arc<StoreIndexWriter>>,
    /// Mirrors pnpm's `verify-store-integrity` / `verifyStoreIntegrity`
    /// setting. When `true` (pnpm's default) each cached CAFS file is
    /// stat'ed and optionally re-hashed before reuse. When `false` the
    /// index is trusted and the import fails lazily if a blob is
    /// missing — trades the per-file stat / optional rehash for the
    /// risk that a mutated or corrupt store serves stale content until
    /// the next integrity-full install. Whether that translates into a
    /// wall-time win depends on the workload; the per-snapshot stat
    /// isn't the bottleneck on the benchmarks this repo tracks (see
    /// [#273]), but cutting the syscall count is still correct.
    ///
    /// [#273]: https://github.com/pnpm/pacquet/issues/273
    pub verify_store_integrity: bool,
    /// Mirrors pnpm's `strictStorePkgContentCheck` setting (default
    /// `true`). A store row whose bundled manifest names a package other
    /// than the row's key does fails the install under it, and is used
    /// with a warning without it. See
    /// [`pnpm_store_dir::pkg_content_mismatch`] for what counts as a
    /// disagreement.
    pub strict_store_pkg_content_check: bool,
    /// Install-scoped dedup cache shared across every cached-tarball
    /// lookup. Ports pnpm's `verifiedFilesCache: Set<string>`: a CAFS
    /// path that one snapshot's verify pass has already stat'ed (and
    /// optionally re-hashed) gets skipped when the next snapshot
    /// touches the same blob. Without it pacquet was paying the
    /// per-file stat in `check_pkg_files_integrity` once per
    /// (snapshot × file) instead of once per (file). Allocate one
    /// `Arc<DashSet<PathBuf>>` at install bootstrap and pass the same
    /// handle to every [`DownloadTarballToStore`].
    pub verified_files_cache: SharedVerifiedFilesCache,
    /// Expected hash of the tarball bytes. `None` for a lockfile entry
    /// recording no `integrity`, the shape pnpm wrote for git-host
    /// archives before it pinned their hash; pnpm fetches those
    /// unverified, so pacquet does too.
    ///
    /// [`Self::run_without_mem_cache`] neither reads nor writes an
    /// `index.db` row for an unpinned archive. Its fallback key belongs
    /// to the *prepared* file set that `GitHostedTarballFetcher` writes
    /// after running `prepare` + packlist; claiming it here would leave
    /// the raw archive in the row whenever that pass failed.
    /// [`Self::fetch_and_extract`] can instead index a plain archive by
    /// its computed integrity.
    pub package_integrity: Option<&'a Integrity>,
    pub package_unpacked_size: Option<usize>,
    /// `dist.fileCount` when the registry published one. Combined with
    /// `package_unpacked_size` into the download's queueing priority —
    /// per-file pipeline overhead (CAS write syscalls, hashing) makes a
    /// many-small-files package as slow to finish as a much larger
    /// few-files one.
    pub package_file_count: Option<usize>,
    pub package_url: &'a str,
    /// Stable identifier for the package, e.g. `"{name}@{version}"`. Paired
    /// with `package_integrity` to form the `SQLite` index key per pnpm v11's
    /// `storeIndexKey`, when there is an integrity to pair it with.
    pub package_id: &'a str,
    /// URL-keyed `Authorization` header lookup, built from the parsed
    /// `.npmrc` creds. Resolved per request so a tarball served from a
    /// different host than the registry still picks up its own header.
    pub auth_headers: &'a AuthHeaders,
    /// Install root the fetch belongs to. Threaded into the
    /// `pnpm:progress` `requester` field on `fetched` /
    /// `found_in_store` events. Same value as the
    /// [`pnpm_reporter::StageLog::prefix`] computed in
    /// `Install::run`.
    pub requester: &'a str,
    /// Pre-fetched cache lookups built once at install start
    /// ([`crate::prefetch::prefetch_cas_paths`]). When `Some`, this is consulted first;
    /// the per-snapshot `SQLite` + integrity-check round-trip is skipped
    /// for every key already resolved by the prefetch.
    pub prefetched_cas_paths: Option<&'a PrefetchedCasPaths>,
    /// Per-attempt retry budget for the tarball pipeline, driven by
    /// pnpm's `fetch-retries*` knobs: every failure retries except
    /// HTTP 401, 403, 404 — including arbitrary 4xx / 5xx, network
    /// resets, timeouts, mid-stream body errors, integrity mismatches,
    /// and gzip / tar parse failures ([#259]).
    ///
    /// [#259]: https://github.com/pnpm/pacquet/issues/259
    pub retry_opts: RetryOpts,
    /// Per-package archive-entry filter applied during CAS extraction.
    /// Receives the entry's path *after* the top-level
    /// `package/` strip; returning `true` drops the entry before the
    /// CAS write, implementing the `ignoreFilePattern` /
    /// `archiveFilters` behavior.
    /// `None` (the default for ordinary npm tarballs) writes every
    /// regular-file entry; `Some(filter)` is what the binary fetcher
    /// uses to strip Node's bundled `npm` / `corepack` from the CAS.
    ///
    /// Stored as `Arc` so the install dispatcher (Slice D) can
    /// construct one filter per fetch from runtime config — e.g.
    /// `archiveFilters` keyed by `pkg.name` — without leaking
    /// memory or pinning the filter to `'static`. Cloning the
    /// Arc per retry attempt is cheap; the inner trait object
    /// is shared.
    pub ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
    /// `offline` from `Config`. When `true` and both the warm
    /// prefetch (`prefetched_cas_paths`) and the `SQLite` `index.db`
    /// lookup (`load_cached_cas_paths`) miss, the fetcher fails fast
    /// with [`TarballError::NoOfflineTarball`] rather than hitting
    /// the registry. The `--offline` flag gates the metadata-fetch
    /// path in pnpm; pacquet has no metadata-fetch path on the
    /// frozen-install flow (the lockfile pins every resolution), so
    /// this gate is pacquet's most useful interpretation of the flag
    /// for frozen installs.
    pub offline: bool,
    /// Install-scoped set used to de-duplicate package-status progress.
    /// When `Some`, a `fetched` or `found_in_store` emit records its
    /// `store_index_key(integrity, pkg_id)` here. Later callers that see
    /// the same key skip their own package-status emit, while still doing
    /// the underlying fetch/cache work. Only the fresh install path
    /// threads this set through, because resolve-time prefetches can
    /// otherwise report the same package again in the warm batch.
    pub progress_reported: Option<SharedReportedProgressKeys>,
    /// Synthesized `package.json` to fold into the freshly extracted
    /// archive, mirroring pnpm's `appendManifest`. Runtime archives
    /// (Node.js / Bun / Deno) ship no manifest of their own, so without
    /// this the store-index row records no `package.json`: every later
    /// *warm* materialization then lands a manifest-less slot, and the
    /// warm-batch bin linker (which reads `PackageFilesIndex.manifest`)
    /// links no bin. When `Some`, the bytes are written to the CAFS and
    /// recorded in the row's `files` map and bundled `manifest` before
    /// the row is queued, so warm and cold installs see the same slot.
    /// `None` (ordinary registry/tarball packages) is a no-op — they
    /// carry their own `package.json`. See `apply_append_manifest`.
    pub append_manifest: Option<&'a [u8]>,
}

/// Project [`TarballError`] onto pnpm's `requestRetryLogger`'s
/// JS-shaped error object. The JS default-reporter dispatches on
/// `httpStatusCode ?? status ?? errno ?? code` to render the retry
/// reason; absent fields skip rather than emit `null` so the `??`
/// chain doesn't short-circuit on a present-but-`null` field.
///
/// Today pacquet populates `http_status_code` for the
/// [`TarballError::HttpStatus`] variant and a curated
/// `ERR_PNPM_*` constant in `code` for every other variant —
/// the mapping is hand-maintained per match arm rather than
/// reflectively derived, so renaming a [`TarballError`] variant
/// won't silently change the emitted `code`. `errno` and `status`
/// are skipped because pacquet's error layer doesn't carry them;
/// pnpm's emit fills them when the underlying network error did.
pub(crate) fn tarball_error_to_request_retry(err: &TarballError) -> RequestRetryError {
    let mut out = RequestRetryError {
        message: err.to_string(),
        http_status_code: None,
        status: None,
        errno: None,
        code: None,
    };
    match err {
        TarballError::HttpStatus(http) => {
            out.http_status_code = Some(http.status.to_string());
        }
        TarballError::FetchTarball(_) => {
            out.code = Some("ERR_PNPM_FETCH".to_string());
        }
        TarballError::OffAllowlist { .. } => {
            out.code = Some("ERR_PNPM_REGISTRY_OFF_ALLOWLIST".to_string());
        }
        TarballError::Checksum(_) => {
            out.code = Some("ERR_PNPM_TARBALL_INTEGRITY".to_string());
        }
        TarballError::DecodeGzip(_) => {
            out.code = Some("ERR_PNPM_TARBALL_GZIP".to_string());
        }
        TarballError::ReadTarballEntries(_) => {
            out.code = Some("ERR_PNPM_TARBALL_TAR".to_string());
        }
        TarballError::ParseBundledManifest { .. } => {
            out.code = Some("ERR_PNPM_TARBALL_EXTRACT".to_string());
        }
        TarballError::ReadLocalTarball { .. } => {
            out.code = Some("ERR_PNPM_TARBALL_FILE".to_string());
        }
        TarballError::WriteCasFile(_) | TarballError::WriteStoreIndex(_) => {
            out.code = Some("ERR_PNPM_TARBALL_STORE".to_string());
        }
        TarballError::TaskJoin(_) => {
            out.code = Some("ERR_PNPM_TASK_JOIN".to_string());
        }
        TarballError::TarballTooLarge { .. } => {
            out.code = Some("ERR_PNPM_TARBALL_TOO_LARGE".to_string());
        }
        TarballError::SiblingFetchFailed { .. } => {
            out.code = Some("ERR_PNPM_SIBLING_FETCH".to_string());
        }
        TarballError::PathTraversal { .. } => {
            out.code = Some("ERR_PNPM_PATH_TRAVERSAL".to_string());
        }
        TarballError::ReadZipArchive { .. } | TarballError::ReadZipEntries { .. } => {
            out.code = Some("ERR_PNPM_ZIP".to_string());
        }
        TarballError::NoOfflineTarball { .. } => {
            // The retry classifier sees this only if the offline gate
            // were ever placed inside the retry loop (it isn't —
            // `NoOfflineTarball` short-circuits before
            // `fetch_and_extract_with_retry`). The arm exists for
            // exhaustiveness; the `code` field is set so a future
            // surface that does run this error through the retry
            // logger renders the right code.
            out.code = Some("ERR_PNPM_NO_OFFLINE_TARBALL".to_string());
        }
        TarballError::UnexpectedPkgContentInStore { .. } => {
            // Same "for exhaustiveness" stance as the arm above: the
            // store read this comes from happens before the retry loop,
            // and re-reading the same row would only reproduce it.
            out.code = Some("ERR_PNPM_UNEXPECTED_PKG_CONTENT_IN_STORE".to_string());
        }
    }
    out
}

/// Whether a [`TarballError`] from one tarball-fetch attempt should be
/// retried.
///
/// We retry integrity mismatches and decode errors. The body fetch
/// *and* the post-download integrity check + extraction live in one
/// retried closure for the same reason: a corrupted byte on the wire
/// that happens to escape TCP framing can break either the integrity
/// check or the gzip decode, and a re-fetch is the cheapest way out.
pub(crate) fn is_transient_error(err: &TarballError) -> bool {
    match err {
        TarballError::HttpStatus(http) => !matches!(http.status, 401 | 403 | 404),
        TarballError::ReadLocalTarball { .. } => false,
        // A route policy does not change between attempts.
        TarballError::OffAllowlist { .. } => false,
        _ => true,
    }
}

pub(crate) async fn extract_tarball_buffer(
    buffer: Vec<u8>,
    expected_integrity: Option<&Integrity>,
    package_unpacked_size: Option<usize>,
    package_url: &str,
    store_dir: &'static StoreDir,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
    let _post_download_permit = post_download_semaphore()
        .acquire()
        .await
        .expect("post-download semaphore shouldn't be closed this soon");

    tracing::info!(target: "pacquet::download", ?package_url, "Download completed");

    let expected_integrity = expected_integrity.cloned();
    let package_url_owned = package_url.to_string();
    let result = tokio::task::spawn_blocking(
        move || -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
            let integrity = verify_tarball_integrity(
                &buffer,
                expected_integrity,
                package_url_owned,
            )?;
            let (cas_paths, pkg_files_idx) = extract_gzipped_tarball(
                &buffer,
                package_unpacked_size,
                store_dir,
                ignore_file_pattern.as_deref(),
            )?;
            Ok((integrity, cas_paths, pkg_files_idx))
        },
    )
    .await
    .map_err(TarballError::TaskJoin)??;

    tracing::info!(target: "pacquet::download", ?package_url, "Checksum verified");

    Ok(result)
}

pub(crate) fn verify_tarball_integrity(
    buffer: &[u8],
    expected_integrity: Option<Integrity>,
    package_url: String,
) -> Result<Integrity, TarballError> {
    if let Some(expected) = expected_integrity {
        expected.check(buffer).map_err(|error| {
            TarballError::Checksum(VerifyChecksumError { url: package_url, error })
        })?;
        return Ok(expected);
    }

    let mut opts = IntegrityOpts::new().algorithm(Algorithm::Sha512);
    opts.input(buffer);
    Ok(opts.result())
}

/// Hashes a tarball body chunk by chunk as it arrives: verifying the
/// pinned integrity when the resolution carries one, computing a fresh
/// sha512 when it does not.
///
/// Same two outcomes [`verify_tarball_integrity`] produces over a fully
/// buffered body. Doing it incrementally is what lets a body be
/// extracted while it downloads without the whole of it being held to
/// hash at the end.
enum BodyHasher {
    Pinned { expected: Integrity, checker: IntegrityChecker },
    Computed(IntegrityOpts),
}

impl BodyHasher {
    fn new(expected_integrity: Option<&Integrity>) -> Self {
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
/// want: [`fetch_and_extract_once`] takes this path up front for a body
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
async fn extract_body_while_downloading<Reporter, Body, Guard>(
    seed: Vec<bytes::Bytes>,
    mut stream: Body,
    mut hasher: BodyHasher,
    progress: &mut BodyProgress<'_>,
    network_permit: Guard,
    http_client: &ThrottledClient,
    package_url: &str,
    store_dir: &'static StoreDir,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError>
where
    Reporter: self::Reporter,
    Body: Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
{
    let network_error =
        |error| TarballError::FetchTarball(NetworkError { url: package_url.to_string(), error });

    let (chunk_tx, chunk_rx) = body_chunk_channel();
    let extractor_ignore = ignore_file_pattern.clone();
    let extract_task = tokio::task::spawn_blocking(move || {
        stream_extract_gzipped_channel(chunk_rx, store_dir, extractor_ignore.as_deref())
    });
    // The body is hashed to its end no matter what the extractor does
    // because the integrity verdict covers every byte. The extractor
    // can legitimately finish early when the tar terminator and gzip
    // trailer arrive while chunks are still in flight. `feed` only
    // stops the sends; a send failure is not an error verdict.
    let mut feed = true;
    for chunk in seed {
        hasher.input(&chunk);
        if feed && chunk_tx.send(Ok(chunk)).await.is_err() {
            feed = false;
        }
    }
    let mut body_error: Option<TarballError> = None;
    while body_error.is_none() {
        match stream.next().await {
            Some(Ok(chunk)) => {
                hasher.input(&chunk);
                progress.on_chunk::<Reporter>(chunk.len());
                if feed && chunk_tx.send(Ok(chunk)).await.is_err() {
                    feed = false;
                }
            }
            Some(Err(error)) => {
                if feed {
                    let _ = chunk_tx
                        .send(Err(std::io::Error::other("the tarball body failed mid-download")))
                        .await;
                }
                body_error = Some(network_error(error));
            }
            None => break,
        }
    }
    if body_error.is_none() {
        progress.warn_if_slow(http_client, package_url);
    }
    // Close the channel so the extractor sees end-of-stream. Release
    // the network permit before waiting on CPU work because the body is
    // done or abandoned after a network error.
    drop(chunk_tx);
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

/// Emits download progress for both body paths of [`fetch_and_extract_once`].
///
/// Mirrors `lodash.throttle(opts.onProgress, 500)` on pnpm's side,
/// down to the leading and trailing edges. The size gate exists
/// because the default reporter renders a percent gauge: without a
/// `Content-Length` there is no denominator, and for a typical
/// sub-megabyte package the gauge would reach 100% before any UI tick
/// could show it.
struct BodyProgress<'a> {
    emit: bool,
    started_at: Instant,
    last_emit: Option<Instant>,
    last_emitted_downloaded: u64,
    downloaded: u64,
    package_id: &'a str,
}

impl<'a> BodyProgress<'a> {
    const BIG_TARBALL_SIZE: u64 = 5 * 1024 * 1024;
    const IN_PROGRESS_THROTTLE: Duration = Duration::from_millis(500);

    fn new(expected_size: Option<u64>, package_id: &'a str) -> Self {
        Self {
            emit: expected_size.is_some_and(|size| size >= Self::BIG_TARBALL_SIZE),
            started_at: Instant::now(),
            last_emit: None,
            last_emitted_downloaded: 0,
            downloaded: 0,
            package_id,
        }
    }

    fn on_chunk<Reporter: self::Reporter>(&mut self, len: usize) {
        self.downloaded = self.downloaded.saturating_add(len as u64);
        let throttle_ready =
            self.last_emit.is_none_or(|instant| instant.elapsed() >= Self::IN_PROGRESS_THROTTLE);
        if self.emit && throttle_ready {
            Reporter::emit(&LogEvent::FetchingProgress(FetchingProgressLog {
                level: LogLevel::Debug,
                message: FetchingProgressMessage::InProgress {
                    downloaded: self.downloaded,
                    package_id: self.package_id.to_owned(),
                },
            }));
            self.last_emit = Some(Instant::now());
            self.last_emitted_downloaded = self.downloaded;
        }
    }

    fn finish<Reporter: self::Reporter>(&mut self) {
        // Match the trailing edge of `lodash.throttle` so consumers
        // observe the final byte count when the last window is partial.
        if self.emit && self.downloaded != self.last_emitted_downloaded {
            Reporter::emit(&LogEvent::FetchingProgress(FetchingProgressLog {
                level: LogLevel::Debug,
                message: FetchingProgressMessage::InProgress {
                    downloaded: self.downloaded,
                    package_id: self.package_id.to_owned(),
                },
            }));
            self.last_emitted_downloaded = self.downloaded;
        }
    }

    fn warn_if_slow(&self, http_client: &ThrottledClient, package_url: &str) {
        if let Some(message) = slow_download_warning(
            self.downloaded,
            self.started_at.elapsed(),
            http_client.fetch_min_speed_ki_bps(),
            package_url,
        ) {
            http_client.warn(&message);
        }
    }
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

/// Run one full tarball-fetch attempt: network, body, integrity hash,
/// decompress, extract into the CAFS. Returns the cas-paths map and the
/// [`PackageFilesIndex`] row the caller queues once the retry loop
/// succeeds.
///
/// One attempt spans the whole pipeline so that a post-download failure
/// — integrity mismatch, gzip decode, malformed tar — reaches the retry
/// boundary, where a re-fetch can recover from a transfer that happened
/// to checksum or decode wrong.
///
/// Permits are acquired inside the attempt so a backoff sleep never
/// parks one. The network permit spans connect through body streaming
/// (pnpm's pQueue, and [#281]'s EMFILE fix) and is dropped before
/// [`post_download_semaphore`] gates the CPU-bound tail.
///
/// [#281]: https://github.com/pnpm/pacquet/pull/281
#[expect(clippy::too_many_arguments, reason = "arg count is fixed by the fetcher signature")]
pub(crate) async fn fetch_and_extract_once<Reporter: self::Reporter>(
    http_client: &ThrottledClient,
    package_url: &str,
    expected_integrity: Option<&Integrity>,
    package_unpacked_size: Option<usize>,
    download_priority: u64,
    package_id: &str,
    attempt: u32,
    store_dir: &'static StoreDir,
    auth_headers: &AuthHeaders,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
    revision_addressed: bool,
) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
    let network_error =
        |error| TarballError::FetchTarball(NetworkError { url: package_url.to_string(), error });

    if let Some(path) = local_file_tarball_path(package_url) {
        let (file, size) = open_local_tarball(&path).await?;
        Reporter::emit(&LogEvent::FetchingProgress(FetchingProgressLog {
            level: LogLevel::Debug,
            message: FetchingProgressMessage::Started {
                attempt: attempt + 1,
                package_id: package_id.to_owned(),
                size: Some(size),
            },
        }));
        let buffer = read_local_tarball_buffer(file, &path, package_url, size).await?;
        return extract_tarball_buffer(
            buffer,
            expected_integrity,
            package_unpacked_size,
            package_url,
            store_dir,
            ignore_file_pattern,
        )
        .await;
    }

    // Acquire the network permit *before* `connect + send` and hold it
    // through body streaming. Releasing earlier would let the next
    // batch of futures `connect()` while previous bodies are still
    // draining, breaking the bound on concurrent open sockets.
    //
    // `acquire_for_url_with_priority` routes the request through the
    // per-registry TLS-configured client when one is set for
    // `package_url`'s nerf-darted prefix, falling back to the default
    // client otherwise. Tarball hosts that differ from the metadata
    // host still pick up the right per-registry client because the
    // 5-step `pickSettingByUrl` lookup also matches on the tarball
    // URL. When the pool is saturated, the package with the most
    // estimated pipeline work claims the next freed slot, so the
    // longest download+extract jobs never start last.
    // The route policy decides whether this origin may be reached at all,
    // at the fetch rather than when the request that named it was read.
    if !auth_headers.allows_fetch(package_url) {
        return Err(TarballError::OffAllowlist {
            url: pnpm_network::redact_url_credentials(package_url),
        });
    }
    let client = if revision_addressed {
        http_client
            .acquire_for_url_without_redirects_with_priority(package_url, download_priority)
            .await
    } else {
        http_client.acquire_for_url_with_priority(package_url, download_priority).await
    };
    let mut request = client.get(package_url);
    // Resolve the per-URL auth header and attach it. Tarball hosts that
    // differ from the metadata host still pick up the header keyed at
    // the registry's nerf-darted URI.
    if let Some(value) = auth_header_for_package_download(auth_headers, package_url, package_id) {
        request = request.header("authorization", value);
    }

    // `pnpm:fetching-progress started` fires exactly once per HTTP
    // attempt — including attempts that fail before the response head
    // arrives (DNS / connect / timeout) so retried attempts stay
    // visible in the reporter.
    // `size` is the response's `Content-Length` when we have a
    // response head, and JSON `null` (i.e. `None`) when we don't:
    // either because the response is chunked / unknown-length, or
    // because the request errored out before headers. pnpm's
    // reporter checks `size != null` before rendering a percent
    // gauge, so this admits "we don't know yet" only when we truly
    // don't know.
    //
    // `attempt` is one-indexed (the in-flight attempt) to match the
    // reporter's wire shape, which expects a 1-indexed counter.
    // Pacquet's loop counter is zero-indexed, so emit `attempt + 1`.
    // The default reporter filters big-tarball progress on
    // `attempt == 1` (so retries don't reset the progress line), so a
    // zero would silence every "Downloading ..." line.
    let send_result = request.send().await;
    let size = send_result.as_ref().ok().and_then(reqwest::Response::content_length);
    Reporter::emit(&LogEvent::FetchingProgress(FetchingProgressLog {
        level: LogLevel::Debug,
        message: FetchingProgressMessage::Started {
            attempt: attempt + 1,
            package_id: package_id.to_owned(),
            size,
        },
    }));
    let response_head = send_result.map_err(network_error)?;

    let status = response_head.status();
    if !status.is_success() {
        // Drain small error bodies so reqwest/hyper can return the
        // connection to the keep-alive pool — dropping an unconsumed
        // `Response` closes the underlying connection, which we'd then
        // pay to reopen on retry. Skip the drain when the body is
        // unknown-length or larger than the cap, since hyper only
        // returns the connection to the pool once the body is fully
        // consumed; a partial drain wouldn't help and would just buffer
        // a pathological response.
        const DRAIN_CAP: u64 = 64 * 1024;
        if response_head.content_length().is_some_and(|len| len <= DRAIN_CAP) {
            let _ = response_head.bytes().await;
        }
        return Err(TarballError::HttpStatus(HttpStatusError {
            url: package_url.to_string(),
            status: status.as_u16(),
        }));
    }

    let expected_size = response_head.content_length();

    let mut stream = response_head.bytes_stream();
    let mut progress = BodyProgress::new(expected_size, package_id);

    // Pull chunks until the gzip magic is decidable. The selected body
    // path receives the prefix so no bytes are consumed twice.
    let mut prefix: Vec<bytes::Bytes> = Vec::new();
    let mut prefix_len = 0usize;
    while prefix_len < GZIP_MAGIC.len() {
        let Some(chunk) = stream.next().await else { break };
        let chunk = chunk.map_err(network_error)?;
        prefix_len += chunk.len();
        prefix.push(chunk);
    }
    let is_gzip = {
        let mut magic = prefix.iter().flat_map(|chunk| chunk.iter().copied());
        (magic.next(), magic.next()) == (Some(GZIP_MAGIC[0]), Some(GZIP_MAGIC[1]))
    };
    // Take the streaming extractor up front when the advertised size
    // already says the archive is large. Retries stay buffered so their
    // terminal errors retain the whole-archive decode's diagnostics.
    if is_gzip
        && attempt == 0
        && expected_size.is_some_and(|size| size >= STREAM_EXTRACT_DURING_DOWNLOAD_THRESHOLD)
        && let Ok(_streaming_permit) = streaming_extract_semaphore().try_acquire()
    {
        for chunk in &prefix {
            progress.on_chunk::<Reporter>(chunk.len());
        }
        return extract_body_while_downloading::<Reporter, _, _>(
            prefix,
            stream,
            BodyHasher::new(expected_integrity),
            &mut progress,
            client,
            http_client,
            package_url,
            store_dir,
            ignore_file_pattern,
        )
        .await;
    }

    let buffer = {
        // Pre-size from the advertised length, but only as far as this
        // path will ever fill: past the threshold below the body is
        // handed to the streaming extractor, so reserving for a larger
        // advertised size would be reserving for bytes that never land
        // here — and would let a server's claim, rather than its body,
        // decide the size of an allocation.
        let reserve =
            expected_size.map(|size| size.min(STREAM_EXTRACT_COMPRESSED_THRESHOLD as u64));
        let mut buf = allocate_tarball_buffer(reserve, package_url)?;
        for chunk in prefix {
            buf.extend_from_slice(&chunk);
            progress.on_chunk::<Reporter>(chunk.len());
        }
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(network_error)?;
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
            if is_gzip {
                // Extract from here on. The archive is decoded in full
                // either way, so no download is refused for being large.
                //
                // The buffer's capacity has doubled past what arrived;
                // hand the extractor the bytes, not the headroom.
                buf.shrink_to_fit();
                let _streaming_permit = streaming_extract_semaphore()
                    .acquire()
                    .await
                    .expect("streaming-extract semaphore shouldn't be closed this soon");
                return extract_body_while_downloading::<Reporter, _, _>(
                    vec![bytes::Bytes::from(buf)],
                    stream,
                    BodyHasher::new(expected_integrity),
                    &mut progress,
                    client,
                    http_client,
                    package_url,
                    store_dir,
                    ignore_file_pattern,
                )
                .await;
            }
            // A body that does not start with the gzip magic fails at
            // the decoder however much of it arrives, so the rest is
            // read and dropped rather than kept. It still has to be
            // read: when the resolution pins an integrity, a body that
            // does not hash to it is a tampered or stale download, and
            // saying so outranks saying it did not decode.
            let mut hasher = BodyHasher::new(expected_integrity);
            hasher.input(&buf);
            drop(buf);
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(network_error)?;
                hasher.input(&chunk);
                progress.on_chunk::<Reporter>(chunk.len());
            }
            hasher.finish(package_url)?;
            return Err(non_gzip_body_error(prefix_len));
        }
        progress.warn_if_slow(http_client, package_url);
        progress.finish::<Reporter>();
        buf
    };
    drop(stream);

    // Body fully buffered; release the network permit before the
    // CPU-bound work so spawn_blocking doesn't hold one of the
    // limited fetch slots.
    //
    // The network permit was the only gate during fetch + body
    // buffering — `default_network_concurrency()` bounds concurrent
    // open sockets and concurrent in-progress fetches. The buffer
    // lives in RAM across this drop and the next acquire, so a
    // pathologically slow decompression stage could let buffered
    // tarballs accumulate beyond the network bound. In practice
    // flate2 decompresses faster than the network delivers, so
    // buffered-but-not-yet-decompressing tarballs stay close to zero.
    // Gating body buffering with `post_download_semaphore` (the
    // smaller `num_cpus * 2` cap) instead would pin `network_concurrency`
    // permits waiting for it and collapse fetch concurrency down to
    // `post_download` — that's the regression `perf(tarball)` (a43ca32)
    // fixed; don't reintroduce it.
    drop(client);

    extract_tarball_buffer(
        buffer,
        expected_integrity,
        package_unpacked_size,
        package_url,
        store_dir,
        ignore_file_pattern,
    )
    .await
}

/// Emit `pnpm:progress found_in_store` for a (`package_id`, requester)
/// pair the cache resolved without a download.
pub(crate) fn emit_progress_found_in_store<Reporter: self::Reporter>(
    package_id: &str,
    requester: &str,
    progress_key: Option<(&SharedReportedProgressKeys, &str)>,
) {
    if progress_already_reported(progress_key) {
        return;
    }
    Reporter::emit(&LogEvent::Progress(ProgressLog {
        level: LogLevel::Debug,
        message: ProgressMessage::FoundInStore {
            package_id: package_id.to_owned(),
            requester: requester.to_owned(),
        },
    }));
}

pub(crate) fn emit_progress_fetched<Reporter: self::Reporter>(
    package_id: &str,
    requester: &str,
    progress_key: Option<(&SharedReportedProgressKeys, &str)>,
) {
    if progress_already_reported(progress_key) {
        return;
    }
    Reporter::emit(&LogEvent::Progress(ProgressLog {
        level: LogLevel::Debug,
        message: ProgressMessage::Fetched {
            package_id: package_id.to_owned(),
            requester: requester.to_owned(),
        },
    }));
}

pub(crate) fn progress_already_reported(
    progress_key: Option<(&SharedReportedProgressKeys, &str)>,
) -> bool {
    progress_key.is_some_and(|(reported, key)| !reported.insert(key.to_owned()))
}

/// Byte-equivalent cost of one file's fixed pipeline overhead (the
/// CAS-write syscalls and hash setup paid per file regardless of its
/// size, ~75 µs against a pipeline that moves a byte through
/// download + decompress + hash + write in ~25 ns). Folding it into
/// the priority makes a many-small-files package rank as the long
/// job it actually is: extraction cost, not just transfer cost,
/// decides when a package's pipeline work finishes.
pub(crate) const PRIORITY_BYTES_PER_FILE: u64 = 3_000;

/// Queueing priority of a tarball download: the package's estimated
/// total pipeline work (transfer + decompress + hash + CAS writes) in
/// byte-equivalents. Missing hints contribute zero, so a package with
/// no published `dist` stats queues behind every estimated one.
#[must_use]
pub fn download_priority(unpacked_size: Option<usize>, file_count: Option<usize>) -> u64 {
    let size = unpacked_size.map_or(0, |size| size as u64);
    let per_file =
        file_count.map_or(0, |count| (count as u64).saturating_mul(PRIORITY_BYTES_PER_FILE));
    // `UNPRIORITIZED` and `BACKGROUND` are class sentinels; a hostile
    // registry publishing absurd `dist` stats must not be able to
    // saturate a download's priority into either class.
    size.saturating_add(per_file).min(MAX_THROUGHPUT_PRIORITY)
}

/// Run [`fetch_and_extract_once`] under pnpm's retry policy. Permanent
/// errors (HTTP 401 / 403 / 404 — see [`is_transient_error`]) fail on
/// the first attempt; everything else sleeps with exponential backoff
/// and tries again until the budget is exhausted, surfacing the most
/// recent error.
///
/// On retry, CAFS writes from a previous attempt that may have made it
/// part-way through extraction stay on disk. That's safe: the CAFS is
/// content-addressed, so re-extracting the same bytes produces
/// identical paths and `write_cas_file` is idempotent.
// 13 arguments — over the default clippy threshold but each is
// distinct: client + URL + integrity describe the request, ID +
// requester are the reporter dimensions, progress_key dedups the
// package-status emit, unpacked-size is allocation hinting,
// download_priority is queue ordering, store_dir + retry_opts +
// auth_headers are install-scoped, and ignore_file_pattern is the
// per-fetch archive filter, and revision_addressed selects the immutable
// request policy. Bundling into a struct would just push
// the same fields into a wrapper.
#[expect(
    clippy::too_many_arguments,
    reason = "the parameters are independent install-scoped inputs; bundling them into a struct only moves the same fields into a wrapper"
)]
pub(crate) async fn fetch_and_extract_with_retry<Reporter: self::Reporter>(
    http_client: &ThrottledClient,
    package_url: &str,
    expected_integrity: Option<&Integrity>,
    package_unpacked_size: Option<usize>,
    download_priority: u64,
    package_id: &str,
    requester: &str,
    store_dir: &'static StoreDir,
    retry_opts: RetryOpts,
    auth_headers: &AuthHeaders,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
    progress_key: Option<(&SharedReportedProgressKeys, &str)>,
    revision_addressed: bool,
) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
    let max_retries = if revision_addressed { 0 } else { retry_opts.retries };
    let mut attempt: u32 = 0;
    loop {
        let result = fetch_and_extract_once::<Reporter>(
            http_client,
            package_url,
            expected_integrity,
            package_unpacked_size,
            download_priority,
            package_id,
            attempt,
            store_dir,
            auth_headers,
            ignore_file_pattern.clone(),
            revision_addressed,
        )
        .await;
        match result {
            Ok(value) => {
                // `pnpm:progress fetched`: one event per (resolved)
                // package once the tarball has been pulled from the
                // network and extracted.
                emit_progress_fetched::<Reporter>(package_id, requester, progress_key);
                return Ok(value);
            }
            Err(err) if !is_transient_error(&err) => return Err(err),
            Err(err) if attempt >= max_retries => {
                tracing::warn!(
                    target: "pacquet::download",
                    ?package_url,
                    attempts = attempt + 1,
                    ?err,
                    "Tarball fetch retry budget exhausted",
                );
                return Err(err);
            }
            Err(err) => {
                let delay = retry_opts.delay_for(attempt);
                tracing::warn!(
                    target: "pacquet::download",
                    ?package_url,
                    attempt = attempt + 1,
                    max_attempts = max_retries + 1,
                    ?delay,
                    ?err,
                    "Tarball fetch failed; retrying after backoff",
                );
                // `pnpm:request-retry`: one event per
                // failed-and-being-retried HTTP attempt, before the
                // backoff sleep, so the JS reporter renders "Will retry
                // in <ms>. <N> retries left." while pacquet is still
                // waiting. `attempt` is one-indexed (the failed
                // attempt) to match the reporter's wire shape;
                // pacquet's loop counter is zero-indexed.
                Reporter::emit(&LogEvent::RequestRetry(RequestRetryLog {
                    level: LogLevel::Debug,
                    attempt: attempt + 1,
                    error: tarball_error_to_request_retry(&err),
                    max_retries,
                    method: "GET".to_string(),
                    timeout: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    url: package_url.to_string(),
                }));
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }
    }
}

/// Store-index key a tarball fetch reads and writes its
/// [`PackageFilesIndex`] row at, or `None` when the resolution carries
/// no integrity to address the row by. See
/// [`DownloadTarballToStore::package_integrity`].
pub(crate) fn store_index_cache_key(
    package_integrity: Option<&Integrity>,
    package_id: &str,
) -> Option<String> {
    package_integrity.map(|integrity| store_index_key(&integrity.to_string(), package_id))
}
