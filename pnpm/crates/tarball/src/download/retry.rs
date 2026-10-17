//! How a failed fetch attempt is classified and reported.

use super::{RequestRetryError, TarballError, is_permanent_error};

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
        TarballError::UnpackedSizeExceeded { .. } => "ERR_PNPM_TARBALL_UNPACKED_SIZE_EXCEEDED",
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
        TarballError::FetchTarball(network) => !is_permanent_error(&network.error),
        TarballError::ReadLocalTarball { .. } => false,
        TarballError::WriteCasFile(write) => !is_storage_full(write),
        // A route policy does not change between attempts.
        TarballError::OffAllowlist { .. } => false,
        // A re-fetch yields the same archive with the same unpacked size.
        TarballError::UnpackedSizeExceeded { .. } => false,
        _ => true,
    }
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
