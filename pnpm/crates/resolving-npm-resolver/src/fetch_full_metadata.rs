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
//! demands it.

use pacquet_network::{
    AuthHeaders, RetryOpts, ThrottledClient, ThrottledClientGuard, redact_url_credentials,
    retry_async, send_with_retry,
};
use pacquet_registry::Package;
use reqwest::{Response, StatusCode, header};

use crate::{FetchMetadataError, mirror::clear_meta, registry_url::to_registry_url};

/// Accept header for the full packument.
pub(crate) const ACCEPT_FULL_DOC: &str = "application/json; q=1.0, */*";

/// Accept header for the abbreviated `install-v1` packument.
pub(crate) const ACCEPT_ABBREVIATED_DOC: &str =
    "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*";

/// Content type of an abbreviated (install-oriented) packument. A
/// spec-compliant registry echoes this in the response `Content-Type` when it
/// honors the abbreviated `Accept` header. Its absence signals that the
/// registry ignored the header and served the full document instead.
/// <https://github.com/npm/registry/blob/main/docs/responses/package-metadata.md>
const ABBREVIATED_META_CONTENT_TYPE: &str = "application/vnd.npm.install-v1+json";

/// Whether the response `Content-Type` declares the abbreviated packument
/// media type. Parameters (`; charset=utf-8`) are dropped and the comparison
/// is case-insensitive (RFC 9110 §8.3.1).
pub(crate) fn is_abbreviated_content_type(headers: &header::HeaderMap) -> bool {
    headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).is_some_and(|value| {
        let media_type = value.split_once(';').map_or(value, |(media_type, _)| media_type);
        media_type.trim().eq_ignore_ascii_case(ABBREVIATED_META_CONTENT_TYPE)
    })
}

/// Options bundle for [`fetch_full_metadata`]. The cached variant
/// ([`crate::FetchFullMetadataCachedOptions`]) layers a
/// cache-directory field on top of these.
#[derive(Debug, Clone)]
pub struct FetchFullMetadataOptions<'a> {
    pub registry: &'a str,
    pub http_client: &'a ThrottledClient,
    pub auth_headers: &'a AuthHeaders,
    /// `true` requests the full packument (with `time`, `_npmUser`,
    /// and `dist.attestations`); `false` requests the abbreviated
    /// `install-v1` form.
    pub full_metadata: bool,
    /// Optional `If-None-Match` header value. When `Some`, the
    /// registry can answer the request with `304 Not Modified` and
    /// the fetcher returns [`FetchFullMetadataOutcome::NotModified`]
    /// instead of a body.
    pub etag: Option<&'a str>,
    /// Optional `If-Modified-Since` header value. Same role as
    /// [`Self::etag`] — gives the registry a chance to short-circuit
    /// the body re-download.
    pub modified: Option<&'a str>,
    pub retry_opts: RetryOpts,
}

/// Outcome of a [`fetch_full_metadata`] call. The caller (today: only
/// `maybe_upgrade_abbreviated_meta_for_release_age` inside
/// [`crate::pick_package()`]) reacts differently to a 304 than to
/// a 200. [`Package`] is boxed so the size of the enum stays small
/// even though a full packument can be many KB — the same boxing
/// pattern used elsewhere in the crate when a large struct sits next
/// to a unit variant.
#[derive(Debug, Clone)]
pub enum FetchFullMetadataOutcome {
    /// Registry returned a 2xx with a parsed body.
    Modified(Box<Package>),
    /// Registry returned `304 Not Modified` because the conditional
    /// headers matched. Callers keep the meta they already have.
    NotModified,
}

pub(crate) struct MetadataRequestOptions<'a> {
    pub pkg_name: &'a str,
    pub url: &'a str,
    pub accept: &'a str,
    pub http_client: &'a ThrottledClient,
    pub auth_headers: &'a AuthHeaders,
    pub etag: Option<&'a str>,
    pub modified: Option<&'a str>,
    pub retry_opts: RetryOpts,
}

/// Send a metadata GET, retrying an unsolicited 304 once with intermediary
/// cache reuse disabled. A repeated 304 cannot validate any local body and is
/// reported with the same error in both pnpm implementations.
pub(crate) async fn send_metadata_request<'a>(
    opts: &MetadataRequestOptions<'a>,
) -> Result<(ThrottledClientGuard<'a>, Response), FetchMetadataError> {
    let etag = opts.etag.filter(|value| !value.is_empty());
    let modified = opts.modified.filter(|value| !value.is_empty());
    let has_validator = etag.is_some() || modified.is_some();
    let build_request = |client: &reqwest::Client, bypass_cache: bool| {
        let mut request = client.get(opts.url).header(header::ACCEPT, opts.accept);
        if let Some(value) = opts.auth_headers.for_url_with_package(opts.url, Some(opts.pkg_name)) {
            request = request.header(header::AUTHORIZATION, value);
        }
        if let Some(etag) = etag {
            request = request.header(header::IF_NONE_MATCH, etag);
        }
        if let Some(modified) = modified {
            request = request.header(header::IF_MODIFIED_SINCE, modified);
        }
        if bypass_cache {
            request = request.header(header::CACHE_CONTROL, "no-cache");
        }
        request
    };

    let (client, response) =
        send_with_retry(opts.http_client, opts.url, opts.retry_opts, |client| {
            build_request(client, false)
        })
        .await
        .map_err(|error| FetchMetadataError::Network {
            url: redact_url_credentials(opts.url),
            error,
        })?;
    if response.status() != StatusCode::NOT_MODIFIED || has_validator {
        return Ok((client, response));
    }

    drop(client);
    let (client, response) =
        send_with_retry(opts.http_client, opts.url, opts.retry_opts, |client| {
            build_request(client, true)
        })
        .await
        .map_err(|error| FetchMetadataError::Network {
            url: redact_url_credentials(opts.url),
            error,
        })?;
    if response.status() == StatusCode::NOT_MODIFIED {
        drop(client);
        return Err(FetchMetadataError::NotModifiedWithoutCache {
            pkg_name: opts.pkg_name.to_string(),
        });
    }
    Ok((client, response))
}

/// Fetch the registry metadata document for `pkg_name`. The
/// `full_metadata` flag on [`FetchFullMetadataOptions`] picks
/// between the full and abbreviated packument forms.
pub async fn fetch_full_metadata(
    pkg_name: &str,
    opts: &FetchFullMetadataOptions<'_>,
) -> Result<FetchFullMetadataOutcome, FetchMetadataError> {
    let url = to_registry_url(opts.registry, pkg_name);
    let accept = if opts.full_metadata { ACCEPT_FULL_DOC } else { ACCEPT_ABBREVIATED_DOC };
    retry_async(&url, opts.retry_opts, FetchMetadataError::is_body_retryable, || async {
        let (client, response) = send_metadata_request(&MetadataRequestOptions {
            pkg_name,
            url: &url,
            accept,
            http_client: opts.http_client,
            auth_headers: opts.auth_headers,
            etag: opts.etag,
            modified: opts.modified,
            retry_opts: opts.retry_opts,
        })
        .await?;
        if response.status() == StatusCode::NOT_MODIFIED {
            return Ok(FetchFullMetadataOutcome::NotModified);
        }
        let response = response.error_for_status().map_err(|error| {
            FetchMetadataError::Network { url: redact_url_credentials(&url), error }
        })?;
        let normalize_to_abbreviated =
            !opts.full_metadata && !is_abbreviated_content_type(response.headers());
        let raw_body = response.text().await.map_err(|error| FetchMetadataError::BodyRead {
            url: redact_url_credentials(&url),
            error,
        })?;
        // Body fully buffered — release the connection and its
        // network-concurrency permit, then parse off the reactor: a
        // multi-MB packument parse would otherwise pin a tokio worker
        // and stall every socket it pumps (see
        // `fetch_full_metadata_cached` for the cold-install numbers).
        drop(client);
        let task_url = url.clone();
        let meta = tokio::task::spawn_blocking(move || -> Result<Package, FetchMetadataError> {
            let meta = serde_json::from_str::<Package>(&raw_body).map_err(|error| {
                FetchMetadataError::Decode { url: redact_url_credentials(&task_url), error }
            })?;
            Ok(if normalize_to_abbreviated { normalize_abbreviated_meta(meta) } else { meta })
        })
        .await
        .map_err(|error| FetchMetadataError::ParseTask {
            url: redact_url_credentials(&url),
            error,
        })??;
        Ok(FetchFullMetadataOutcome::Modified(Box::new(meta)))
    })
    .await
}

/// Strip a packument served for an **abbreviated** request down to the
/// abbreviated field set, for a registry that ignored the `Accept` header
/// (detected via [`is_abbreviated_content_type`]) and returned the full
/// document. Neither the in-memory pick nor the on-disk mirror then carries
/// the install-irrelevant data (scripts, exports, readme, custom fields) a
/// full document contains. Registries that honor the header (e.g. the npm
/// registry) echo the abbreviated `Content-Type` and skip this entirely.
///
/// Normalization is a cache optimization, not a correctness requirement, so
/// the practically-unreachable [`clear_meta`] failure (a fragment that was
/// parsed from the response body failing to re-parse) keeps the full
/// document rather than failing the fetch.
pub(crate) fn normalize_abbreviated_meta(meta: Package) -> Package {
    match clear_meta(&meta) {
        Ok(normalized) => normalized,
        Err(error) => {
            tracing::warn!(
                target: "pacquet_resolving_npm_resolver",
                %error,
                "could not normalize a non-abbreviated metadata response; keeping the full document",
            );
            meta
        }
    }
}

#[cfg(test)]
mod tests;
