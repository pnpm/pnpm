use crate::{SharedReportedProgressKeys, TarballError, download::emit_progress_fetched};
use pnpm_network::{RetryOpts, is_permanent_error, redact_url_for_display};
use pnpm_reporter::{LogEvent, LogLevel, Reporter, RequestRetryError, RequestRetryLog};
use std::future::Future;

/// Retry the complete fetch, verification and extraction attempt. Failed
/// attempts cannot publish store rows; successful attempts emit fetched once.
pub(crate) async fn retry_archive<Reporter, Output, Attempt>(
    package_url: &str,
    package_id: &str,
    requester: &str,
    progress_key: Option<(&SharedReportedProgressKeys, &str)>,
    retry_opts: RetryOpts,
    mut fetch: impl FnMut(u32) -> Attempt,
) -> Result<Output, TarballError>
where
    Reporter: self::Reporter,
    Attempt: Future<Output = Result<Output, TarballError>>,
{
    let max_retries = retry_opts.retries;
    let mut attempt: u32 = 0;
    loop {
        let result = fetch(attempt).await;
        match result {
            Ok(value) => {
                emit_progress_fetched::<Reporter>(package_id, requester, progress_key);
                return Ok(value);
            }
            Err(err) if !is_transient_error(&err) => return Err(err),
            Err(err) if attempt >= max_retries => {
                tracing::warn!(
                    target: "pacquet::download",
                    package_url = %redact_url_for_display(package_url),
                    attempts = u64::from(attempt) + 1,
                    %err,
                    "Archive fetch retry budget exhausted",
                );
                return Err(err);
            }
            Err(err) => {
                let delay = retry_opts.delay_for(attempt);
                report_archive_retry::<Reporter>(package_url, &err, attempt, max_retries, delay);
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }
    }
}

fn report_archive_retry<Reporter: self::Reporter>(
    package_url: &str,
    err: &TarballError,
    attempt: u32,
    max_retries: u32,
    delay: std::time::Duration,
) {
    tracing::warn!(
        target: "pacquet::download",
        package_url = %redact_url_for_display(package_url),
        attempt = u64::from(attempt) + 1,
        max_attempts = u64::from(max_retries) + 1,
        ?delay,
        %err,
        "Archive fetch failed; retrying after backoff",
    );
    Reporter::emit(&LogEvent::RequestRetry(RequestRetryLog {
        level: LogLevel::Debug,
        attempt: attempt.saturating_add(1),
        error: tarball_error_to_request_retry(err),
        max_retries,
        method: "GET".to_string(),
        timeout: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
        url: redact_url_for_display(package_url),
    }));
}

/// Project [`TarballError`] onto pnpm's `requestRetryLogger`'s
/// JS-shaped error object. The JS default-reporter dispatches on
/// `httpStatusCode ?? status ?? errno ?? code` to render the retry
/// reason; absent fields skip rather than emit `null` so the `??`
/// chain doesn't short-circuit on a present-but-`null` field.
///
/// The mapping is hand-maintained per match arm rather than
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
        TarballError::HttpStatus(http) => out.http_status_code = Some(http.status.to_string()),
        _ => out.code = retry_error_code(err).map(str::to_string),
    }
    out
}

/// The `code` [`tarball_error_to_request_retry`] reports for `err`. `None` for
/// an HTTP status, which is reported as `httpStatusCode` instead.
fn retry_error_code(err: &TarballError) -> Option<&'static str> {
    let code = match err {
        TarballError::HttpStatus(_) => return None,
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
        TarballError::TarballTooLarge { .. } | TarballError::ArchiveLimit(_) => {
            "ERR_PNPM_TARBALL_TOO_LARGE"
        }
        TarballError::UnsupportedContentEncoding { .. } | TarballError::ContentDecode { .. } => {
            "ERR_PNPM_TARBALL_CONTENT_ENCODING"
        }
        TarballError::UnrecognizedArchive => "ERR_PNPM_TARBALL_UNRECOGNIZED",
        TarballError::IncompleteTar { .. } => "ERR_PNPM_TARBALL_INCOMPLETE_TAR",
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
    Some(code)
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
        TarballError::FetchTarball(network) => !is_permanent_error(&network.error),
        TarballError::ReadLocalTarball { .. } => false,
        TarballError::WriteCasFile(write) => !is_storage_full(write),
        TarballError::ReadTarballEntries(io) if is_permanent_tar_entry_error(io) => false,
        // A route policy does not change between attempts, and neither does
        // what a server sends that pnpm refuses to decode.
        TarballError::OffAllowlist { .. }
        | TarballError::UnsupportedContentEncoding { .. }
        | TarballError::ContentDecode { .. }
        | TarballError::UnrecognizedArchive
        | TarballError::ArchiveLimit(_)
        | TarballError::PathTraversal { .. } => false,
        _ => true,
    }
}

fn is_permanent_tar_entry_error(error: &std::io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| {
            inner.is::<crate::extract::OversizedManifestError>()
                || inner.is::<crate::extract::RejectedEntryPathError>()
        })
}

fn is_storage_full(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut source = Some(error);
    while let Some(error) = source {
        if error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::StorageFull)
        {
            return true;
        }
        source = error.source();
    }
    false
}
