//! Cache-aware metadata fetcher.
//!
//! Ports pnpm's
//! [`fetchFullMetadataCached`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/fetchFullMetadataCached.ts).
//!
//! When a cache directory is configured, the fetcher consults a
//! shared mirror under `<cache_dir>/v11/metadata-full/` (full) or
//! `<cache_dir>/v11/metadata/` (abbreviated), keyed by
//! `full_metadata`. It issues a conditional GET against the upstream
//! registry, and either reads the cached body (304) or writes the
//! new body back (2xx). Without a cache directory it falls through
//! to a plain GET — the same behavior callers got before Phase 5
//! from [`crate::fetch_full_metadata()`].
//!
//! The directory layout matches pnpm's; the file format is pacquet's
//! own indexed shape (see [`crate::mirror`]) so warm loads hydrate
//! only the version fragments a pick consults.

use std::path::Path;

use pacquet_network::{AuthHeaders, RetryOpts, ThrottledClient, send_with_retry};
use pacquet_registry::Package;
use pipe_trait::Pipe;
use reqwest::{StatusCode, header};

use crate::{
    FetchMetadataError,
    fetch_full_metadata::{ACCEPT_ABBREVIATED_DOC, ACCEPT_FULL_DOC},
    mirror::{
        ABBREVIATED_META_DIR, FULL_META_DIR, get_pkg_mirror_path, load_meta_async,
        load_meta_headers_async, save_meta_indexed,
    },
    registry_url::to_registry_url,
};

/// Options bundle for [`fetch_full_metadata_cached`]. Mirrors
/// upstream's `FetchFullMetadataCachedOptions` — same fields, same
/// optionality. `cache_dir` is the only addition over the no-cache
/// [`crate::FetchFullMetadataOptions`].
#[derive(Debug, Clone)]
pub struct FetchFullMetadataCachedOptions<'a> {
    pub registry: &'a str,
    pub http_client: &'a ThrottledClient,
    pub auth_headers: &'a AuthHeaders,
    /// When `Some`, the fetcher consults the on-disk mirror under
    /// the matching `<cache_dir>/v11/metadata...` subdirectory.
    /// When `None`, the fetcher short-circuits to an unconditional
    /// GET.
    pub cache_dir: Option<&'a Path>,
    /// `true` requests the full packument and caches it under
    /// [`FULL_META_DIR`]; `false` requests the abbreviated form
    /// and caches it under [`ABBREVIATED_META_DIR`]. Mirrors
    /// upstream's
    /// [`fetchFullMetadataCached`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/fetchFullMetadataCached.ts#L30-L36)
    /// vs.
    /// [`fetchAbbreviatedMetadataCached`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/fetchFullMetadataCached.ts#L47-L53)
    /// dispatch.
    pub full_metadata: bool,
    pub(crate) retry_opts: RetryOpts,
}

/// Fetch the full registry metadata document for `pkg_name`, reusing
/// the shared on-disk mirror when `cache_dir` is supplied. Ports
/// upstream's
/// [`fetchFullMetadataCached`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/fetchFullMetadataCached.ts#L30-L36).
///
/// Flow:
///
/// 1. **Compute mirror path** (when `cache_dir` is set). Failures
///    in this step degrade silently — a malformed registry URL or
///    other path-encoding error just disables the cache for this
///    call; the fetch still issues an unconditional GET.
/// 2. **Read cache headers** off the mirror's first line via the
///    internal `load_meta_headers` helper. Missing file /
///    unreadable / malformed → no conditional headers; the GET is
///    unconditional.
/// 3. **Issue the GET** with `If-None-Match` /
///    `If-Modified-Since` headers when both ETag/Last-Modified were
///    available, plus the per-URL `Authorization` header from
///    [`AuthHeaders`].
/// 4. **On `304 Not Modified`**: re-read the mirror via the
///    internal `load_meta` helper.
///    A 304 with no mirror present propagates as [`FetchMetadataError::NotModifiedWithoutCache`]
///    (matches upstream's `META_NOT_MODIFIED_WITHOUT_CACHE`);
///    a 304 whose mirror vanishes between the headers read and the
///    full read propagates as [`FetchMetadataError::CacheMissingAfter304`]
///    (matches `META_CACHE_MISSING_AFTER_304`).
/// 5. **On `2xx`**: parse the response into [`Package`], write the
///    body + new headers to the mirror best-effort (a cache-write
///    failure logs at debug but never fails the call — the install
///    proceeds without the speedup on the next run), and return.
/// 6. **On non-2xx / non-304**: surface
///    [`FetchMetadataError::Network`].
pub async fn fetch_full_metadata_cached(
    pkg_name: &str,
    opts: &FetchFullMetadataCachedOptions<'_>,
) -> Result<Package, FetchMetadataError> {
    let meta_dir = if opts.full_metadata { FULL_META_DIR } else { ABBREVIATED_META_DIR };
    // Encoding the mirror path can fail only on a malformed registry
    // URL (no host, unparsable). Either case is a config bug; we
    // log and proceed without a cache so the user still gets metadata
    // on this install instead of a hard error.
    let mirror_path = match opts.cache_dir {
        Some(dir) => match get_pkg_mirror_path(dir, meta_dir, opts.registry, pkg_name) {
            Ok(path) => Some(path),
            Err(error) => {
                tracing::debug!(
                    target: "pacquet_resolving_npm_resolver::cache",
                    ?error,
                    registry = opts.registry,
                    pkg_name,
                    full_metadata = opts.full_metadata,
                    "could not encode mirror path; bypassing cache for this call",
                );
                None
            }
        },
        None => None,
    };
    let cache_headers = load_meta_headers_async(mirror_path.as_deref()).await;

    let url = to_registry_url(opts.registry, pkg_name);
    let accept = if opts.full_metadata { ACCEPT_FULL_DOC } else { ACCEPT_ABBREVIATED_DOC };
    let (client, response) = send_with_retry(opts.http_client, &url, opts.retry_opts, |client| {
        let mut request = client.get(&url).header(header::ACCEPT, accept);
        if let Some(value) = opts.auth_headers.for_url(&url) {
            request = request.header(header::AUTHORIZATION, value);
        }
        if let Some(headers) = cache_headers.as_ref() {
            if let Some(etag) = headers.etag.as_deref() {
                request = request.header(header::IF_NONE_MATCH, etag);
            }
            if let Some(modified) = headers.modified.as_deref() {
                request = request.header(header::IF_MODIFIED_SINCE, modified);
            }
        }
        request
    })
    .await
    .map_err(|error| FetchMetadataError::Network { url: url.clone(), error })?;

    if response.status() == StatusCode::NOT_MODIFIED {
        // No body to stream — release the connection and its
        // network-concurrency permit before the mirror disk read.
        drop(client);
        let Some(path) = mirror_path else {
            // 304 without an existing cache to fall back on — the
            // registry over-reached on `If-None-Match: <stale>`.
            // Mirrors upstream's `META_NOT_MODIFIED_WITHOUT_CACHE`.
            return Err(FetchMetadataError::NotModifiedWithoutCache {
                pkg_name: pkg_name.to_string(),
            });
        };
        return load_meta_async(Some(&path)).await.ok_or_else(|| {
            FetchMetadataError::CacheMissingAfter304 { pkg_name: pkg_name.to_string() }
        });
    }

    let response = response
        .error_for_status()
        .map_err(|error| FetchMetadataError::Network { url: url.clone(), error })?;

    let etag = response
        .headers()
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let raw_body = response
        .text()
        .await
        .map_err(|error| FetchMetadataError::Network { url: url.clone(), error })?;

    // Body fully buffered — release the connection and its
    // network-concurrency permit before the CPU-bound parse so the
    // semaphore keeps bounding *sockets*, not parses. Same
    // buffer-then-release shape as the tarball pipeline in
    // `pacquet-tarball`.
    drop(client);

    // Deserialize and persist the mirror off the reactor. Packuments
    // run to several megabytes for high-release-cadence packages
    // (`@fluentui/*`, `@types/node`, ...); parsing one inline pins a
    // tokio worker for hundreds of milliseconds and stalls every
    // socket that worker pumps — on a cold babylon install the
    // inline parses held the metadata phase to a third of pnpm's
    // throughput.
    let task_url = url.clone();
    let meta = tokio::task::spawn_blocking(move || -> Result<Package, FetchMetadataError> {
        let meta: Package = serde_json::from_str(&raw_body)
            .map_err(|error| FetchMetadataError::Decode { url: task_url, error })?;

        if let Some(path) = mirror_path.as_deref() {
            // The lazily-parsed `meta` still borrows every version
            // fragment from `raw_body`, so the indexed write streams
            // the registry's own bytes — no re-serialization.
            if let Err(error) = save_meta_indexed(path, &meta, etag.as_deref()) {
                // Fire-and-forget — a read-only cache dir or a
                // shared-store contention shouldn't fail the
                // install. The user just won't see the warm-cache
                // speedup next time.
                tracing::debug!(
                    target: "pacquet_resolving_npm_resolver::cache",
                    ?error,
                    path = %path.display(),
                    "could not persist mirror; bypassing cache write",
                );
            }
        }
        Ok(meta)
    })
    .await
    .map_err(|error| FetchMetadataError::ParseTask { url, error })??;

    meta.pipe(Ok)
}

#[cfg(test)]
mod tests;
