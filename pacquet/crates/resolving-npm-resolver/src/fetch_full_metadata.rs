//! No-cache metadata fetcher.
//!
//! Issues a GET against `<registry>/<package>`. The `full_metadata`
//! flag picks between the **full** packument
//! (`Accept: application/json; q=1.0, */*`) and the **abbreviated**
//! install-v1 form
//! (`Accept: application/vnd.npm.install-v1+json; q=1.0,
//! application/json; q=0.8, */*`). The two forms are byte-different:
//! the abbreviated document drops the per-version `time` map,
//! `_npmUser`, and `dist.attestations`, which are exactly the fields
//! the `minimumReleaseAge` and `trustPolicy='no-downgrade'` checks
//! read.
//!
//! Callers that need maturity / trust evidence (the verifier and the
//! resolver's upgrade-on-recent-modified path) must request full
//! metadata. The resolver's default install path requests
//! abbreviated and upgrades to full only when the maturity check
//! demands it — mirroring upstream's pickPackage logic.
//!
//! Ports the request half of upstream's
//! [`fetchMetadataFromFromRegistry`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/fetch.ts#L118-L204).

use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_registry::Package;

use crate::{FetchMetadataError, registry_url::to_registry_url};

/// Accept header for the full packument. Matches upstream's
/// [`ACCEPT_FULL_DOC`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/network/fetch/src/fetchFromRegistry.ts#L12).
pub(crate) const ACCEPT_FULL_DOC: &str = "application/json; q=1.0, */*";

/// Accept header for the abbreviated `install-v1` packument. Matches
/// upstream's
/// [`ACCEPT_ABBREVIATED_DOC`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/network/fetch/src/fetchFromRegistry.ts#L15).
pub(crate) const ACCEPT_ABBREVIATED_DOC: &str =
    "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*";

/// Options bundle for [`fetch_full_metadata`]. Mirrors upstream's
/// `FetchFullMetadataCachedOptions` minus the cache fields; the
/// cached variant layers them on.
#[derive(Debug, Clone)]
pub struct FetchFullMetadataOptions<'a> {
    pub registry: &'a str,
    pub http_client: &'a ThrottledClient,
    pub auth_headers: &'a AuthHeaders,
    /// `true` requests the full packument (with `time`, `_npmUser`,
    /// and `dist.attestations`); `false` requests the abbreviated
    /// `install-v1` form. Mirrors upstream's
    /// [`opts.fullMetadata`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/fetch.ts#L113).
    pub full_metadata: bool,
}

/// Fetch the registry metadata document for `pkg_name`. The
/// `full_metadata` flag on [`FetchFullMetadataOptions`] picks
/// between the full and abbreviated packument forms.
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
    let accept = if opts.full_metadata { ACCEPT_FULL_DOC } else { ACCEPT_ABBREVIATED_DOC };
    let mut request =
        opts.http_client.acquire_for_url(&url).await.get(&url).header("accept", accept);
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
