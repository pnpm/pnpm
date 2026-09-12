//! Downloading a tarball into the content-addressable store.
//!
//! Owns the retry policy, integrity verification, and the progress
//! events the reporter renders during a fetch.

pub(crate) use body::{BodyProgress, slow_download_warning};
pub(crate) use fetch::fetch_and_extract_once;
pub use progress::download_priority;
pub(crate) use progress::{emit_progress_fetched, emit_progress_found_in_store};

use super::{
    Arc, Duration, GZIP_MAGIC, HashMap, IgnoreEntryFilter, Instant, NetworkError, Path, PathBuf,
    PrefetchedCasPaths, STREAM_EXTRACT_COMPRESSED_THRESHOLD,
    STREAM_EXTRACT_DURING_DOWNLOAD_THRESHOLD, SharedReportedProgressKeys, TarballError,
    VerifyChecksumError, allocate_tarball_buffer, body_chunk_channel, extract_gzipped_tarball,
    local_file_tarball_path, non_gzip_body_error, open_local_tarball, post_download_semaphore,
    read_local_tarball_buffer, stream_extract_gzipped_channel, streaming_extract_semaphore,
};
use crate::{extract::BodyChunkSender, extraction_task::spawn_extraction};
use futures_util::{Stream, StreamExt};
use pnpm_network::{
    AuthHeaders, MAX_THROUGHPUT_PRIORITY, RetryOpts, ThrottledClient, redact_url_for_display,
};
use pnpm_reporter::{
    FetchingProgressLog, FetchingProgressMessage, LogEvent, LogLevel, ProgressLog, ProgressMessage,
    Reporter, RequestRetryError,
};
use pnpm_store_dir::{
    PackageFilesIndex, SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreDir,
    StoreIndexWriter, store_index_key,
};
use ssri::{Algorithm, Integrity, IntegrityChecker, IntegrityOpts};
use tokio::sync::SemaphorePermit;

/// Controls how archive files are projected into pnpm's content-addressable store.
///
/// Package archives get pnpm's `package.json` completion marker and may receive a
/// synthesized manifest. Raw archives preserve their regular-file contents exactly;
/// their ecosystem adapter owns any additional metadata and install layout.
#[derive(Debug, Clone, Copy)]
pub enum ArchiveStoreProjection<'a> {
    /// Project the archive as an npm-compatible package. The archive receives
    /// pnpm's completion marker when it has no `package.json`; runtime archives
    /// may additionally supply the manifest that should be synthesized.
    Package { append_manifest: Option<&'a [u8]> },
    /// Preserve the archive's regular files without adding npm package files.
    /// The ecosystem adapter owns all post-ingestion metadata and layout.
    RawArchive,
}

impl<'a> ArchiveStoreProjection<'a> {
    /// Ordinary package keys stay byte-for-byte compatible with pnpm's existing
    /// store. Projections that change the archive's file set use separate
    /// namespaces so they cannot reuse an incompatible row.
    #[must_use]
    pub fn store_index_key(self, integrity: &str, package_id: &str) -> String {
        let base_key = store_index_key(integrity, package_id);
        match self {
            Self::Package { append_manifest: None } => base_key,
            Self::Package { append_manifest: Some(manifest) } => {
                format!("package-manifest\t{}\t{base_key}", manifest_integrity(manifest))
            }
            Self::RawArchive => format!("raw-archive\t{base_key}"),
        }
    }

    /// Ordinary package keys stay byte-for-byte compatible with the URL keys
    /// inserted by resolve-time fetches. Only projections that can produce a
    /// different file set receive a discriminator; synthesized manifests are
    /// content-addressed so equal projections still share work.
    pub(crate) fn mem_cache_key(self, package_url: &str, revision_addressed: bool) -> String {
        match (self, revision_addressed) {
            (Self::Package { append_manifest: None }, false) => package_url.to_string(),
            (Self::Package { append_manifest: None }, true) => {
                format!("revision-addressed:{package_url}")
            }
            (Self::RawArchive, false) => format!("raw-archive:{package_url}"),
            (Self::RawArchive, true) => format!("revision-addressed:raw-archive:{package_url}"),
            (Self::Package { append_manifest: Some(manifest) }, revision_addressed) => {
                let revision_prefix = if revision_addressed { "revision-addressed:" } else { "" };
                format!(
                    "{revision_prefix}package-manifest:{}:{package_url}",
                    manifest_integrity(manifest),
                )
            }
        }
    }

    pub(crate) fn package_content_check(self, strict: bool) -> PackageContentCheck {
        match self {
            Self::Package { .. } if strict => PackageContentCheck::Strict,
            Self::Package { .. } => PackageContentCheck::Warn,
            Self::RawArchive => PackageContentCheck::Skip,
        }
    }

    pub(crate) fn legacy_synthesized_store_row(
        self,
        integrity: &str,
        package_id: &str,
    ) -> Option<(String, &'a [u8])> {
        match self {
            Self::Package { append_manifest: Some(manifest) } => {
                Some((store_index_key(integrity, package_id), manifest))
            }
            Self::Package { append_manifest: None } | Self::RawArchive => None,
        }
    }
}

fn manifest_integrity(manifest: &[u8]) -> Integrity {
    let mut opts = IntegrityOpts::new().algorithm(Algorithm::Sha256);
    opts.input(manifest);
    opts.result()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackageContentCheck {
    Strict,
    Warn,
    Skip,
}

/// `Clone` is cheap — every field is a reference, a `Copy` scalar, or an
/// `Arc` — so a caller can keep a copy to retry through a different entry
/// point (e.g. fall back to [`Self::run_without_mem_cache`] after a
/// best-effort [`Self::run_with_mem_cache`] reports a sibling failure).
#[derive(Clone)]
#[must_use]
pub struct IngestTarballToStore<'a> {
    pub http_client: &'a ThrottledClient,
    pub store_dir: &'static StoreDir,
    /// Shared read-only handle to the `SQLite` store index. `None` when the
    /// store does not (yet) have an `index.db`, in which case every cache
    /// lookup short-circuits to a network fetch. Callers open this once per
    /// install and pass the same handle to every [`IngestTarballToStore`]
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
    /// handle to every [`IngestTarballToStore`].
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
    /// Ecosystem-owned projection policy applied after verified extraction and
    /// before the store-index row is queued.
    pub store_projection: ArchiveStoreProjection<'a>,
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
    let code = match err {
        TarballError::HttpStatus(http) => {
            out.http_status_code = Some(http.status.to_string());
            return out;
        }
        TarballError::FetchTarball(_) => "ERR_PNPM_FETCH",
        TarballError::OffAllowlist { .. } => "ERR_PNPM_REGISTRY_OFF_ALLOWLIST",
        TarballError::Checksum(_) => "ERR_PNPM_TARBALL_INTEGRITY",
        TarballError::DecodeGzip(_) => "ERR_PNPM_TARBALL_GZIP",
        TarballError::ReadTarballEntries(_) => "ERR_PNPM_TARBALL_TAR",
        TarballError::ParseBundledManifest { .. } => "ERR_PNPM_TARBALL_EXTRACT",
        TarballError::ReadLocalTarball { .. } => "ERR_PNPM_TARBALL_FILE",
        TarballError::WriteCasFile(_) | TarballError::WriteStoreIndex(_) => {
            "ERR_PNPM_TARBALL_STORE"
        }
        TarballError::TaskJoin(_) => "ERR_PNPM_TASK_JOIN",
        TarballError::TarballTooLarge { .. } => "ERR_PNPM_TARBALL_TOO_LARGE",
        TarballError::SiblingFetchFailed { .. } => "ERR_PNPM_SIBLING_FETCH",
        TarballError::PathTraversal { .. } => "ERR_PNPM_PATH_TRAVERSAL",
        TarballError::ReadZipArchive { .. } | TarballError::ReadZipEntries { .. } => "ERR_PNPM_ZIP",
        TarballError::NoOfflineTarball { .. } => {
            // The retry classifier sees this only if the offline gate
            // were ever placed inside the retry loop (it isn't —
            // `NoOfflineTarball` short-circuits before
            // `fetch_and_extract_with_retry`). The arm exists for
            // exhaustiveness; the `code` field is set so a future
            // surface that does run this error through the retry
            // logger renders the right code.
            "ERR_PNPM_NO_OFFLINE_TARBALL"
        }
        TarballError::UnexpectedPkgContentInStore { .. } => {
            // Same "for exhaustiveness" stance as the arm above: the
            // store read this comes from happens before the retry loop,
            // and re-reading the same row would only reproduce it.
            "ERR_PNPM_UNEXPECTED_PKG_CONTENT_IN_STORE"
        }
    };
    out.code = Some(code.to_string());
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
    let post_download_permit = post_download_semaphore()
        .acquire()
        .await
        .expect("post-download semaphore shouldn't be closed this soon");

    tracing::info!(target: "pacquet::download", ?package_url, "Download completed");

    let expected_integrity = expected_integrity.cloned();
    let package_url_owned = package_url.to_string();
    let result = spawn_extraction(
        post_download_permit,
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

impl<'a> BodyProgress<'a> {
    const BIG_TARBALL_SIZE: u64 = 5 * 1024 * 1024;
    const IN_PROGRESS_THROTTLE: Duration = Duration::from_millis(500);

    pub(crate) fn new(expected_size: Option<u64>, package_id: &'a str) -> Self {
        Self {
            emit: expected_size.is_some_and(|size| size >= Self::BIG_TARBALL_SIZE),
            started_at: Instant::now(),
            last_emit: None,
            last_emitted_downloaded: 0,
            downloaded: 0,
            package_id,
        }
    }

    pub(crate) fn on_chunks<Reporter: self::Reporter>(&mut self, chunks: &[bytes::Bytes]) {
        for chunk in chunks {
            self.on_chunk::<Reporter>(chunk.len());
        }
    }

    pub(crate) fn on_chunk<Reporter: self::Reporter>(&mut self, len: usize) {
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

    pub(crate) fn finish<Reporter: self::Reporter>(&mut self) {
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
    crate::archive_retry::retry_archive::<Reporter, _, _>(
        package_url,
        package_id,
        requester,
        progress_key,
        RetryOpts {
            retries: if revision_addressed { 0 } else { retry_opts.retries },
            ..retry_opts
        },
        |attempt| {
            fetch_and_extract_once::<Reporter>(
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
        },
    )
    .await
}

/// Store-index key a tarball fetch reads and writes its
/// [`PackageFilesIndex`] row at, or `None` when the resolution carries
/// no integrity to address the row by. See
/// [`IngestTarballToStore::package_integrity`].
pub(crate) fn store_index_cache_key(
    package_integrity: Option<&Integrity>,
    package_id: &str,
    store_projection: ArchiveStoreProjection<'_>,
) -> Option<String> {
    package_integrity
        .map(|integrity| store_projection.store_index_key(&integrity.to_string(), package_id))
}

mod body;
use body::{
    BodyHasher, BufferBody, Buffered, advertises_large_body, buffer_body,
    extract_body_while_downloading, starts_with_gzip_magic,
};

mod fetch;

use fetch::fetch_error;

mod progress;
