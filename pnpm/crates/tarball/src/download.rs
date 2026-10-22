//! Downloading a tarball into the content-addressable store.
//!
//! Owns the retry policy, integrity verification, and the progress
//! events the reporter renders during a fetch.

pub(crate) use body::{BodyProgress, slow_download_warning};
pub(crate) use buffered::{BufferedArchive, ExtractedFiles, extract_tarball_buffer};
pub(crate) use fetch::{AttemptedFetch, ExtractedArchive, fetch_and_extract_once};
pub use progress::download_priority;
pub(crate) use progress::{emit_progress_fetched, emit_progress_found_in_store};

use super::{
    Arc, Duration, HashMap, IgnoreEntryFilter, Instant, MAX_UNTRUSTED_PREALLOC_BYTES, NetworkError,
    Path, PathBuf, STREAM_EXTRACT_COMPRESSED_THRESHOLD, STREAM_EXTRACT_DURING_DOWNLOAD_THRESHOLD,
    SharedReportedProgressKeys, TarLimits, TarballError, VerifyChecksumError,
    allocate_tarball_buffer, body_chunk_channel,
    content_coding::ContentCoding,
    extract::{BodyChunkSender, ExtractedResource, StreamedResource},
    extract_archive_bytes, extract_resource_stream,
    extraction_task::spawn_extraction,
    local_file_tarball_path, open_local_tarball, post_download_semaphore,
    read_local_tarball_buffer, streaming_extract_semaphore,
};
use crate::archive_integrity::ArchiveVerification;
use futures_util::{Stream, StreamExt};
use pnpm_network::{
    AuthHeaders, MAX_THROUGHPUT_PRIORITY, RetryOpts, ThrottledClient, redact_url_for_display,
};
use pnpm_reporter::{
    FetchingProgressLog, FetchingProgressMessage, LogEvent, LogLevel, ProgressLog, ProgressMessage,
    Reporter,
};
use pnpm_store_dir::{PackageFilesIndex, StoreDir, store_index_key};
use ssri::{Algorithm, Integrity, IntegrityOpts};
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

    /// Identity of the archive a shared [`crate::MemCache`] slot holds. See
    /// [`package_mem_cache_key`], the shape every caller outside this crate
    /// uses.
    pub(crate) fn mem_cache_key(
        self,
        package_url: &str,
        integrity: Option<&Integrity>,
        revision_addressed: bool,
    ) -> String {
        let projection = match self {
            Self::Package { append_manifest: None } => "package".to_string(),
            Self::Package { append_manifest: Some(manifest) } => {
                format!("package-manifest:{}", manifest_integrity(manifest))
            }
            Self::RawArchive => "raw-archive".to_string(),
        };
        let network_policy = if revision_addressed { "revision-addressed" } else { "direct" };
        let integrity = integrity.map_or_else(String::new, Integrity::to_string);
        format!("{projection}\t{network_policy}\t{integrity}\t{package_url}")
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

/// Cache identity of an ordinary package archive, the projection every
/// resolve-time publisher, speculative prefetch and install-time lookup
/// has to agree on.
///
/// The expected hash is part of it: a slot's bytes were verified against
/// one integrity, and a request pinning a different one must not be served
/// them. An archive whose hash the fetch discovers is published under the
/// computed hash, which is the one its resolution then records. The network
/// policy is part of it too, since a revision-addressed fetch and a direct
/// one are different requests for the same URL. Projections that can produce
/// a different file set carry their own discriminator; synthesized manifests
/// are content-addressed so equal projections still share work.
///
/// The parts are tab-separated, and neither a URL, an integrity, nor a
/// projection tag can contain a tab, so two distinct identities cannot spell
/// the same key.
#[must_use]
pub fn package_mem_cache_key(
    package_url: &str,
    integrity: Option<&Integrity>,
    revision_addressed: bool,
) -> String {
    ArchiveStoreProjection::Package { append_manifest: None }.mem_cache_key(
        package_url,
        integrity,
        revision_addressed,
    )
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
    pub fetching: crate::ArchiveFetchOptions<'a>,
    pub package: crate::TarballPackage<'a>,
    pub store: crate::ArchiveStoreContext<'a>,
    /// Install root the fetch belongs to. Threaded into the
    /// `pnpm:progress` `requester` field on `fetched` /
    /// `found_in_store` events. Same value as the
    /// [`pnpm_reporter::StageLog::prefix`] computed in
    /// `Install::run`.
    pub requester: &'a str,
    /// Per-package archive-entry filter applied during CAS extraction.
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
    /// Install-scoped set used to de-duplicate package-status progress.
    /// Only the fresh install path threads this set through, because
    /// resolve-time prefetches can otherwise report the same package
    /// again in the warm batch.
    pub progress_reported: Option<SharedReportedProgressKeys>,
    /// Ecosystem-owned projection policy applied after verified extraction and
    /// before the store-index row is queued.
    pub store_projection: ArchiveStoreProjection<'a>,
}

pub(crate) fn verify_tarball_integrity(
    buffer: &[u8],
    expected_integrity: Option<Integrity>,
    package_url: String,
) -> Result<Integrity, TarballError> {
    if let Some(expected) = expected_integrity {
        expected
            .check(buffer)
            .map_err(|error| {
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
/// errors (see [`super::archive_retry::is_transient_error`]) fail on the first attempt;
/// transient errors sleep with exponential backoff until the retry
/// budget is exhausted, surfacing the most recent error.
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
pub(crate) async fn fetch_and_extract_archive_with_retry<Reporter: self::Reporter>(
    http_client: &ThrottledClient,
    package_url: &str,
    verification: &ArchiveVerification,
    package_unpacked_size: Option<usize>,
    tar_limits: TarLimits,
    download_priority: u64,
    package_id: &str,
    requester: &str,
    store_dir: &'static StoreDir,
    retry_opts: RetryOpts,
    auth_headers: &AuthHeaders,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
    progress_key: Option<(&SharedReportedProgressKeys, &str)>,
    revision_addressed: bool,
) -> Result<ExtractedFiles, TarballError> {
    match fetch_and_extract_conditional::<Reporter>(
        http_client,
        package_url,
        verification,
        package_unpacked_size,
        tar_limits,
        download_priority,
        package_id,
        requester,
        store_dir,
        retry_opts,
        auth_headers,
        ignore_file_pattern,
        progress_key,
        revision_addressed,
        None,
    )
    .await?
    {
        AttemptedFetch::Extracted(extracted) => {
            let extracted = *extracted;
            Ok((extracted.integrity, extracted.files, extracted.index, extracted.bins))
        }
        AttemptedFetch::NotModified(meta) => {
            Err(TarballError::HttpStatus(crate::HttpStatusError {
                url: meta.final_url,
                status: 304,
            }))
        }
    }
}

/// Fetch and extract a tarball with retry, allowing a conditional
/// `If-None-Match` to resolve to [`AttemptedFetch::NotModified`] instead of a
/// body.
#[expect(
    clippy::too_many_arguments,
    reason = "the parameters are independent install-scoped inputs; bundling them into a struct only moves the same fields into a wrapper"
)]
pub(crate) async fn fetch_and_extract_conditional<Reporter: self::Reporter>(
    http_client: &ThrottledClient,
    package_url: &str,
    verification: &ArchiveVerification,
    package_unpacked_size: Option<usize>,
    tar_limits: TarLimits,
    download_priority: u64,
    package_id: &str,
    requester: &str,
    store_dir: &'static StoreDir,
    retry_opts: RetryOpts,
    auth_headers: &AuthHeaders,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
    progress_key: Option<(&SharedReportedProgressKeys, &str)>,
    revision_addressed: bool,
    if_none_match: Option<&str>,
) -> Result<AttemptedFetch, TarballError> {
    let retries = if revision_addressed { 0 } else { retry_opts.retries };
    crate::archive_retry::retry_archive::<Reporter, _, _>(
        package_url,
        package_id,
        requester,
        progress_key,
        RetryOpts { retries, ..retry_opts },
        |attempt| {
            fetch_and_extract_once::<Reporter>(
                http_client,
                package_url,
                verification,
                package_unpacked_size,
                tar_limits,
                download_priority,
                package_id,
                attempt,
                store_dir,
                auth_headers,
                ignore_file_pattern.clone(),
                revision_addressed,
                if_none_match,
            )
        },
    )
    .await
}

/// Store-index key a tarball fetch reads and writes its
/// [`PackageFilesIndex`] row at, or `None` when the resolution carries
/// no integrity to address the row by. See
/// [`crate::TarballPackage::integrity`].
pub(crate) fn store_index_cache_key(
    package_integrity: Option<&Integrity>,
    package_id: &str,
    store_projection: ArchiveStoreProjection<'_>,
) -> Option<String> {
    package_integrity.map(|integrity| {
        store_projection.store_index_key(&integrity.to_string(), package_id)
    })
}

mod body;
use body::{
    BodyVerification, BufferBody, Buffered, buffer_body, extract_body_while_downloading,
    streams_while_downloading,
};

mod buffered;

mod fetch;

use fetch::fetch_error;

mod progress;
