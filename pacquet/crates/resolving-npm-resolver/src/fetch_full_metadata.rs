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
//! [`crate::create_npm_resolution_verifier()`] consumes today; the
//! cached variant wraps it without changing the call site.
//!
//! Ports the no-cache half of upstream's
//! [`fetchFullMetadataCached.ts`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/fetchFullMetadataCached.ts).

use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_registry::Package;

use crate::{FetchMetadataError, registry_url::to_registry_url};

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
    // and the error mapper. Mirrors upstream's
    // [`toUri`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/fetch.ts)
    // — scoped names get the `/` after the `@scope` percent-encoded
    // so the registry routes the request to the package as a single
    // path segment, not two.
    let url = to_registry_url(opts.registry, pkg_name);
    let mut request =
        opts.http_client.acquire_for_url(&url).await.get(&url).header("accept", "application/json");
    if let Some(value) = opts.auth_headers.for_url(&url) {
        request = request.header("authorization", value);
    }
    let response = request
        .send()
        .await
        .map_err(|error| FetchMetadataError::Network { url: url.clone(), error })?
        .error_for_status()
        .map_err(|error| FetchMetadataError::Network { url: url.clone(), error })?;
    // Decode in two steps so a JSON-shape mismatch surfaces as
    // `FetchMetadataError::Decode` (with the serde_json error), not
    // as `Network` (which `.json::<T>()` would do, conflating
    // transport and parse failures and losing the
    // `decode_error` diagnostic code).
    let raw_body = response
        .text()
        .await
        .map_err(|error| FetchMetadataError::Network { url: url.clone(), error })?;
    serde_json::from_str(&raw_body)
        .map_err(|error| FetchMetadataError::Decode { url: url.clone(), error })
}

#[cfg(test)]
mod tests;
