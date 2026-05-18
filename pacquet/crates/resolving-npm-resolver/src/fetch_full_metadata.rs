//! No-cache full-metadata fetcher.
//!
//! Issues a GET against `<registry>/<package>` with the *full*
//! metadata `Accept` header (`application/json; q=1.0`) — distinct
//! from the abbreviated `application/vnd.npm.install-v1+json`
//! endpoint that pnpm's resolver uses for fast picks. The verifier
//! needs the full document because the abbreviated form omits the
//! `time` map, `_npmUser`, and `dist.attestations` — the three
//! fields the `minimumReleaseAge` and `trustPolicy='no-downgrade'`
//! checks read.
//!
//! Caching (conditional GETs + on-disk mirror) lands in a follow-up
//! phase. This module is the no-cache baseline that
//! [`crate::create_npm_resolution_verifier`] consumes today; the
//! cached variant wraps it without changing the call site.
//!
//! Ports the no-cache half of upstream's
//! [`fetchFullMetadataCached.ts`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/fetchFullMetadataCached.ts).

use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_registry::Package;
use pipe_trait::Pipe;

use crate::FetchMetadataError;

/// Options bundle for [`fetch_full_metadata`]. Mirrors upstream's
/// `FetchFullMetadataCachedOptions` minus the cache fields, which the
/// cached variant (Phase 5) will layer on.
#[derive(Debug, Clone)]
pub struct FetchFullMetadataOptions<'a> {
    pub registry: &'a str,
    pub http_client: &'a ThrottledClient,
    pub auth_headers: &'a AuthHeaders,
}

/// Fetch the **full** registry metadata document for `pkg_name`.
/// The full document carries `time`, per-version `_npmUser`, and
/// `dist.attestations` — the abbreviated install-v1 endpoint pnpm's
/// resolver normally uses omits all three.
pub async fn fetch_full_metadata(
    pkg_name: &str,
    opts: &FetchFullMetadataOptions<'_>,
) -> Result<Package, FetchMetadataError> {
    // Format once and reuse for the request, the auth-header lookup,
    // and the error mapper. Mirrors the pattern
    // [`Package::fetch_from_registry`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/fetch.ts)
    // ports.
    let url = format!("{registry}{name}", registry = opts.registry, name = pkg_name);
    let mut request =
        opts.http_client.acquire_for_url(&url).await.get(&url).header("accept", "application/json");
    if let Some(value) = opts.auth_headers.for_url(&url) {
        request = request.header("authorization", value);
    }
    request
        .send()
        .await
        .map_err(|error| FetchMetadataError::Network { url: url.clone(), error })?
        .error_for_status()
        .map_err(|error| FetchMetadataError::Network { url: url.clone(), error })?
        .json::<Package>()
        .await
        .map_err(|error| FetchMetadataError::Network { url: url.clone(), error })?
        .pipe(Ok)
}

#[cfg(test)]
mod tests;
