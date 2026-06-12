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

use pacquet_network::{AuthHeaders, RetryOpts, ThrottledClient, send_with_retry};
use pacquet_registry::Package;
use reqwest::{StatusCode, header};

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
/// `FetchFullMetadataCachedOptions` minus the cache-directory field;
/// the cached variant layers it on.
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
    /// Optional `If-None-Match` header value. When `Some`, the
    /// registry can answer the request with `304 Not Modified` and
    /// the fetcher returns [`FetchFullMetadataOutcome::NotModified`]
    /// instead of a body. Mirrors upstream's
    /// [`etag` option](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/fetch.ts#L113)
    /// passed verbatim into the `make-fetch-happen` request.
    pub etag: Option<&'a str>,
    /// Optional `If-Modified-Since` header value. Same role as
    /// [`Self::etag`] — gives the registry a chance to short-circuit
    /// the body re-download. Mirrors upstream's `modified` option at
    /// the same call site.
    pub modified: Option<&'a str>,
    pub(crate) retry_opts: RetryOpts,
}

/// Outcome of a [`fetch_full_metadata`] call. Mirrors upstream's
/// [`FetchMetadataResult | FetchMetadataNotModifiedResult`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/fetch.ts#L80-L86)
/// union — the caller (today: only
/// `maybe_upgrade_abbreviated_meta_for_release_age` inside
/// [`crate::pick_package()`]) reacts differently to a 304 than to
/// a 200. [`Package`] is boxed
/// so the size of the enum stays small even though a full packument
/// can be many KB; mirrors the same boxing pattern used elsewhere in
/// the crate when a large struct sits next to a unit variant.
#[derive(Debug, Clone)]
pub enum FetchFullMetadataOutcome {
    /// Registry returned a 2xx with a parsed body.
    Modified(Box<Package>),
    /// Registry returned `304 Not Modified` because the conditional
    /// headers matched. Callers keep the meta they already have.
    NotModified,
}

/// Fetch the registry metadata document for `pkg_name`. The
/// `full_metadata` flag on [`FetchFullMetadataOptions`] picks
/// between the full and abbreviated packument forms.
///
/// When [`FetchFullMetadataOptions::etag`] or
/// [`FetchFullMetadataOptions::modified`] is set, the request
/// includes `If-None-Match` / `If-Modified-Since` headers and the
/// registry may answer `304 Not Modified` — the fetcher returns
/// [`FetchFullMetadataOutcome::NotModified`] without a body in that
/// case. Mirrors upstream's
/// [`fetchFromRegistry`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/network/fetch/src/fetchFromRegistry.ts#L41-L86)
/// 304 short-circuit, used by
/// [`maybeUpgradeAbbreviatedMetaForReleaseAge`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L488-L499)
/// so the upgrade fetch coalesces against the registry's
/// representation cache.
pub async fn fetch_full_metadata(
    pkg_name: &str,
    opts: &FetchFullMetadataOptions<'_>,
) -> Result<FetchFullMetadataOutcome, FetchMetadataError> {
    // Format once and reuse for the request, the auth-header lookup,
    // and the error mapper. Mirrors upstream's
    // [`toUri`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/fetch.ts)
    // — scoped names get the `/` after the `@scope` percent-encoded
    // so the registry routes the request to the package as a single
    // path segment, not two.
    let url = to_registry_url(opts.registry, pkg_name);
    let accept = if opts.full_metadata { ACCEPT_FULL_DOC } else { ACCEPT_ABBREVIATED_DOC };
    let (client, response) = send_with_retry(opts.http_client, &url, opts.retry_opts, |client| {
        let mut request = client.get(&url).header(header::ACCEPT, accept);
        if let Some(value) = opts.auth_headers.for_url(&url) {
            request = request.header(header::AUTHORIZATION, value);
        }
        if let Some(etag) = opts.etag {
            request = request.header(header::IF_NONE_MATCH, etag);
        }
        if let Some(modified) = opts.modified {
            request = request.header(header::IF_MODIFIED_SINCE, modified);
        }
        request
    })
    .await
    .map_err(|error| FetchMetadataError::Network { url: url.clone(), error })?;
    if response.status() == StatusCode::NOT_MODIFIED {
        return Ok(FetchFullMetadataOutcome::NotModified);
    }
    let response = response
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
    // Body fully buffered — release the connection and its
    // network-concurrency permit, then parse off the reactor: a
    // multi-MB packument parse would otherwise pin a tokio worker
    // and stall every socket it pumps (see
    // `fetch_full_metadata_cached` for the cold-install numbers).
    drop(client);
    let task_url = url.clone();
    let meta = tokio::task::spawn_blocking(move || {
        serde_json::from_str::<Package>(&raw_body)
            .map_err(|error| FetchMetadataError::Decode { url: task_url, error })
    })
    .await
    .map_err(|error| FetchMetadataError::ParseTask { url, error })??;
    Ok(FetchFullMetadataOutcome::Modified(Box::new(meta)))
}

#[cfg(test)]
mod tests;
