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
        ABBREVIATED_META_DIR, FULL_FILTERED_META_DIR, FULL_META_DIR, clear_meta,
        get_pkg_mirror_path, load_meta_async, load_meta_headers_async, save_meta_indexed,
        save_meta_ndjson,
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
    /// When full metadata is requested, use pnpm's filtered metadata
    /// mirror and persist the filtered packument shape.
    pub filter_metadata: bool,
    pub(crate) retry_opts: RetryOpts,
}

/// Fetch the full registry metadata document for `pkg_name`, reusing
/// the shared on-disk mirror when `cache_dir` is supplied. Ports
/// upstream's
/// [`fetchFullMetadataCached`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/fetchFullMetadataCached.ts#L30-L36).
pub async fn fetch_full_metadata_cached(
    pkg_name: &str,
    opts: &FetchFullMetadataCachedOptions<'_>,
) -> Result<Package, FetchMetadataError> {
    let meta_dir = if opts.full_metadata {
        if opts.filter_metadata { FULL_FILTERED_META_DIR } else { FULL_META_DIR }
    } else {
        ABBREVIATED_META_DIR
    };
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
        if let Some(value) = opts.auth_headers.for_url_with_package(&url, Some(pkg_name)) {
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
        let meta = load_meta_async(Some(&path)).await.ok_or_else(|| {
            FetchMetadataError::CacheMissingAfter304 { pkg_name: pkg_name.to_string() }
        })?;
        renew_mirror_freshness(&path);
        return Ok(meta);
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
    let should_filter_metadata = opts.full_metadata && opts.filter_metadata;
    let meta = tokio::task::spawn_blocking(move || -> Result<Package, FetchMetadataError> {
        let mut meta: Package = serde_json::from_str(&raw_body)
            .map_err(|error| FetchMetadataError::Decode { url: task_url.clone(), error })?;
        if should_filter_metadata {
            meta = clear_meta(&meta).map_err(|error| FetchMetadataError::Decode {
                url: task_url.clone(),
                error: error.into_inner(),
            })?;
        }

        if let Some(path) = mirror_path.as_deref() {
            // A filtered full response is written in pnpm's NDJSON
            // shape. Other responses keep pacquet's indexed mirror
            // layout for lazy version hydration.
            let save_result = if should_filter_metadata {
                save_meta_ndjson(path, &meta, etag.as_deref())
            } else {
                save_meta_indexed(path, &meta, etag.as_deref())
            };
            if let Err(error) = save_result {
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

/// Bump the mirror file's mtime to "now" after a `304 Not Modified`.
///
/// The publishedBy mtime shortcut in [`crate::pick_package()`] treats
/// a mirror younger than the maturity cutoff as authoritative. A 304
/// proves the cached packument equals the registry's current one, so
/// the validation clock legitimately restarts here — without the
/// touch, a mirror older than `minimumReleaseAge` re-validates every
/// package on every subsequent install, because a 304 never rewrites
/// the file.
///
/// The append-mode open carries the write-attributes access Windows'
/// `set_modified` needs (a read-only handle cannot set file times
/// there). The read-only fallback covers Unix mirrors whose mode
/// dropped write permission: `futimens`-style timestamp syscalls
/// require ownership, not write access. Best-effort: a failure only
/// costs the next install another conditional request.
fn renew_mirror_freshness(path: &Path) {
    let touched = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .or_else(|_| std::fs::File::open(path))
        .and_then(|file| file.set_modified(std::time::SystemTime::now()));
    if let Err(error) = touched {
        tracing::debug!(
            target: "pacquet_resolving_npm_resolver::cache",
            ?error,
            path = %path.display(),
            "could not renew mirror freshness after 304",
        );
    }
}

#[cfg(test)]
mod tests;
