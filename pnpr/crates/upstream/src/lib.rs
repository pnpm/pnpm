pub use packument::{
    abbreviate_packument, extract_upstream_version_manifest, extract_version_manifest,
    rewrite_tarball_urls, rewrite_upstream_tarball_urls, tarball_basename,
};

pub use oci::oci_download_allowed;

mod circuit_breaker;
use circuit_breaker::CircuitBreaker;

mod packument;

mod oci;

use chrono::{DateTime, Utc};
use pnpm_lockfile::{
    MAX_TARBALL_REVISION, TarballRevision, integrity_addressed_registry_tarball_url,
    is_integrity_addressed_registry_tarball_url,
};
use pnpm_network::{
    RedirectGuard, ThrottledClient, ThrottledClientGuard, ThrottledResponse, UNPRIORITIZED,
    is_url_secure_for_credentials, read_limited_body,
};
use pnpr_config::{RedactedHeaders, UpstreamConfig};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::CanonicalPackageName;
use reqwest::{
    StatusCode,
    header::{self, HeaderMap, HeaderValue},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use ssri::Integrity;
use std::{
    fmt,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const UPSTREAM_ERROR_BODY_LIMIT: usize = 64 * 1024;
const UPSTREAM_DISCOVERY_BODY_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    pub objects: Vec<Value>,
    pub total: usize,
}

/// Wraps a shared [`ThrottledClient`] (so the registry inherits pnpm's
/// tuned reqwest defaults: `User-Agent: pnpm`, HTTP/1.1, capped native DNS,
/// pool/timeout tuning, concurrency semaphore, and per-registry TLS
/// routing if it's ever wired in later) and adds the per-upstream glue a
/// proxy needs: building the upstream URL, applying verdaccio's
/// `timeout`/`max_fails`/`fail_timeout` knobs, and fishing the packument
/// or tarball response out of it.
#[derive(Clone)]
pub struct Upstream {
    client: Arc<ThrottledClient>,
    fetch_guard: Option<RedirectGuard>,
    base: String,
    /// The configured upstream name (the YAML `upstreams:` key). Surfaced in
    /// client-facing errors so an open circuit names the upstream rather
    /// than leaking its upstream URL.
    name: String,
    /// Resolved per-upstream request headers (auth + custom) attached to
    /// every fetch. Empty for an upstream with no `auth:`/`headers:`.
    headers: HeaderMap,
    /// Per-request deadline (verdaccio's `timeout`).
    timeout: Duration,
    /// Per-upstream packument freshness window (verdaccio's `maxage`), or
    /// `None` to defer to the global [`pnpr_config::Config::packument_ttl`].
    maxage: Option<Duration>,
    /// Whether tarballs from this upstream are written to the local mirror
    /// (verdaccio's `cache`).
    cache: bool,
    /// Shared failure tracker implementing verdaccio's
    /// `max_fails`/`fail_timeout` circuit breaker. Behind an [`Arc`] so
    /// every clone of this `Upstream` (the registry holds one per upstream
    /// and clones it per request) updates the same counters.
    breaker: Arc<CircuitBreaker>,
    oci_tokens: Arc<Mutex<std::collections::HashMap<String, oci::CachedToken>>>,
}

impl fmt::Debug for Upstream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Upstream")
            .field("client", &self.client)
            .field("fetch_guard", &self.fetch_guard.is_some())
            .field("base", &self.base)
            .field("name", &self.name)
            .field("headers", &RedactedHeaders(&self.headers))
            .field("timeout", &self.timeout)
            .field("maxage", &self.maxage)
            .field("cache", &self.cache)
            .field("breaker", &self.breaker)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub enum FetchOutcome<Payload> {
    /// Upstream returned content.
    Ok(Payload),
    /// Upstream returned 404. The caller should propagate this verbatim.
    NotFound,
}

/// Conditional-GET validators sent on a packument refresh (an `ETag` /
/// `Last-Modified` replayed as `If-None-Match` / `If-Modified-Since`). A
/// per-registry cache refetches a stale entry rather than revalidating it, so in
/// practice the default (empty) value is always sent; the type stays so the
/// fetch path can grow conditional revalidation again without a signature
/// change.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CacheValidators {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub etag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_modified: Option<String>,
}

/// A packument fetched against an upstream.
/// A document fetched by [`Upstream::fetch_document`].
#[derive(Debug)]
pub struct FetchedDocument {
    pub bytes: Vec<u8>,
    /// The URL the body was served from, after redirects. Relative URLs
    /// inside the document resolve against it.
    pub url: String,
}

/// Whether two URLs share a scheme, host and port, so a credential meant for
/// one may be sent to the other.
fn same_origin(base: &str, url: &str) -> bool {
    let (Ok(base), Ok(url)) = (reqwest::Url::parse(base), reqwest::Url::parse(url)) else {
        return false;
    };
    base.scheme() == url.scheme()
        && base.host_str().is_some_and(|host| Some(host) == url.host_str())
        && base.port_or_known_default() == url.port_or_known_default()
}

#[derive(Debug)]
pub struct FetchedPackument {
    pub bytes: Vec<u8>,
}

/// Outcome of a (possibly conditional) packument fetch.
#[derive(Debug)]
pub enum PackumentFetch {
    /// Upstream returned a fresh body, along with its cache validators.
    Modified(FetchedPackument),
    /// Upstream answered `304 Not Modified`: the validators we sent are
    /// still current, so the caller should keep serving its cached copy.
    NotModified,
    /// Upstream returned 404.
    NotFound,
}

impl Upstream {
    /// Build an upstream client from its name (the YAML `upstreams:` key) and
    /// resolved [`UpstreamConfig`], baking in the per-upstream
    /// `timeout`/`maxage`/`cache` knobs and arming the
    /// `max_fails`/`fail_timeout` circuit breaker.
    #[must_use]
    pub fn new(name: &str, config: &UpstreamConfig) -> Self {
        Self {
            client: Arc::new(ThrottledClient::new_for_installs()),
            fetch_guard: None,
            base: config.url.clone(),
            name: name.to_string(),
            headers: config.headers.clone(),
            timeout: config.timeout,
            maxage: config.maxage,
            cache: config.cache,
            breaker: Arc::new(CircuitBreaker::new(config.max_fails, config.fail_timeout)),
            oci_tokens: Arc::default(),
        }
    }

    /// Restrict initial requests and redirects to operator-approved destinations.
    #[must_use]
    pub fn with_fetch_guard(mut self, guard: RedirectGuard) -> Self {
        let redirect_guard = Arc::clone(&guard);
        self.client = Arc::new(ThrottledClient::new_for_installs_with_redirect_guard(move |url| {
            redirect_guard(url)
        }));
        self.fetch_guard = Some(guard);
        self
    }

    /// Per-upstream packument freshness window (`maxage`), or `None` to
    /// defer to the global [`pnpr_config::Config::packument_ttl`].
    #[must_use]
    pub fn maxage(&self) -> Option<Duration> {
        self.maxage
    }

    /// Whether tarballs from this upstream should be written to the local
    /// mirror (`cache: true`). When `false` the caller streams the body
    /// straight through without caching it.
    #[must_use]
    pub fn caches(&self) -> bool {
        self.cache
    }

    /// Fetch a package's packument, conditionally when `validators`
    /// carries an `ETag`/`Last-Modified`. A `304 Not Modified` short-
    /// circuits to [`PackumentFetch::NotModified`] without a body, so the
    /// caller can keep serving its cached copy — the bandwidth win on a
    /// stale-but-current packument.
    ///
    /// Returns [`RegistryError::UpstreamUnavailable`] without hitting the
    /// network when the circuit breaker is open.
    pub async fn fetch_packument(
        &self,
        name: &CanonicalPackageName,
        validators: &CacheValidators,
    ) -> Result<PackumentFetch> {
        self.ensure_available()?;
        let url = format!("{}/{}", self.base.trim_end_matches('/'), name.as_str());
        let mut conditional_headers = HeaderMap::new();
        if let Some(etag) = &validators.etag
            && let Ok(value) = HeaderValue::from_str(etag)
        {
            conditional_headers.insert(header::IF_NONE_MATCH, value);
        }
        if let Some(last_modified) = &validators.last_modified
            && let Ok(value) = HeaderValue::from_str(last_modified)
        {
            conditional_headers.insert(header::IF_MODIFIED_SINCE, value);
        }
        let sent_conditional = !conditional_headers.is_empty();
        let (response, _guard) = self.get_with_scoped_headers(&url, &conditional_headers).await?;
        if response.status() == StatusCode::NOT_FOUND {
            // A 404 is an authoritative answer, not an upstream failure.
            self.breaker.record_success();
            return Ok(PackumentFetch::NotFound);
        }
        // Only honor a `304` if we actually sent a conditional header. A
        // `304` to an unconditional request is a misbehaving upstream and
        // carries no body to serve, so let it fall through to `checked`
        // and surface as an upstream status error rather than reusing a
        // (possibly nonexistent) cached copy.
        if sent_conditional && response.status() == StatusCode::NOT_MODIFIED {
            self.breaker.record_success();
            return Ok(PackumentFetch::NotModified);
        }
        let response = self.checked(response, &url).await?;
        let bytes = response.bytes().await.map_err(|source| {
            self.breaker.record_failure();
            RegistryError::Upstream { url: url.clone(), source }
        })?;
        self.breaker.record_success();
        Ok(PackumentFetch::Modified(FetchedPackument { bytes: bytes.to_vec() }))
    }

    /// Send the tarball request and return a [`ThrottledResponse`]. Use its
    /// [`bytes_stream`](ThrottledResponse::bytes_stream) to forward the body
    /// with bounded buffering and network permits held until the producer ends.
    /// Status and 404 handling happen before any bytes are forwarded.
    ///
    /// Returns [`RegistryError::UpstreamUnavailable`] without hitting the
    /// network when the circuit breaker is open.
    pub async fn fetch_tarball_response(
        &self,
        name: &CanonicalPackageName,
        filename: &str,
    ) -> Result<FetchOutcome<ThrottledResponse>> {
        let started = Instant::now();
        self.ensure_available()?;
        let url = format!("{}/{}/-/{}", self.base.trim_end_matches('/'), name.as_str(), filename);
        let (response, guard) = self.get_with_scoped_headers(&url, &HeaderMap::new()).await?;
        if response.status() == StatusCode::NOT_FOUND {
            self.breaker.record_success();
            return Ok(FetchOutcome::NotFound);
        }
        let response = self.checked(response, &url).await?;
        // Success here covers only headers; a mid-stream body failure is
        // the caller's to observe. Recording success on a clean status is
        // what verdaccio does too.
        self.breaker.record_success();
        Ok(FetchOutcome::Ok(
            guard.retain_for_body(response, self.timeout.saturating_sub(started.elapsed())),
        ))
    }

    /// Fetch a document by path relative to the upstream's base URL — a Cargo
    /// sparse-index file, a Python Simple API page — buffering at most
    /// `limit` bytes. `accept` sets the request's `Accept` header when the
    /// upstream negotiates a representation (the Simple API's JSON form).
    ///
    /// Returns [`RegistryError::UpstreamUnavailable`] without hitting the
    /// network when the circuit breaker is open.
    pub async fn fetch_document(
        &self,
        relative_path: &str,
        accept: Option<&str>,
        limit: usize,
    ) -> Result<FetchOutcome<FetchedDocument>> {
        self.ensure_available()?;
        let url = format!(
            "{}/{}",
            self.base.trim_end_matches('/'),
            relative_path.trim_start_matches('/'),
        );
        let mut headers = HeaderMap::new();
        if let Some(accept) = accept
            && let Ok(value) = HeaderValue::from_str(accept)
        {
            headers.insert(header::ACCEPT, value);
        }
        let (response, _guard) = self.get_with_scoped_headers(&url, &headers).await?;
        if response.status() == StatusCode::NOT_FOUND {
            self.breaker.record_success();
            return Ok(FetchOutcome::NotFound);
        }
        let response = self.checked(response, &url).await?;
        let final_url = response.url().to_string();
        let body = read_limited_body(response, limit).await.map_err(|err| {
            self.breaker.record_failure();
            RegistryError::UpstreamResponse { url: url.clone(), reason: err.to_string() }
        })?;
        if body.truncated {
            self.breaker.record_success();
            return Err(RegistryError::UpstreamResponse {
                url,
                reason: format!("response body exceeds the {limit}-byte limit"),
            });
        }
        self.breaker.record_success();
        Ok(FetchOutcome::Ok(FetchedDocument { bytes: body.bytes, url: final_url }))
    }

    /// Fetch an artifact from the absolute URL an upstream's metadata
    /// published — a crate archive from the index's `dl` template, a wheel
    /// from a Simple API page. The configured headers travel only when `url`
    /// is on the upstream's own origin: an index may point downloads at a
    /// separate host (crates.io does, so does pypi.org), and the upstream's
    /// credential must never reach it.
    ///
    /// Returns [`RegistryError::UpstreamUnavailable`] without hitting the
    /// network when the circuit breaker is open.
    pub async fn fetch_artifact_response(
        &self,
        url: &str,
    ) -> Result<FetchOutcome<ThrottledResponse>> {
        let started = Instant::now();
        self.ensure_available()?;
        let (response, guard) = self.get_with_scoped_headers(url, &HeaderMap::new()).await?;
        if response.status() == StatusCode::NOT_FOUND {
            self.breaker.record_success();
            return Ok(FetchOutcome::NotFound);
        }
        let response = self.checked(response, url).await?;
        self.breaker.record_success();
        Ok(FetchOutcome::Ok(
            guard.retain_for_body(response, self.timeout.saturating_sub(started.elapsed())),
        ))
    }

    /// Fetch an immutable sha512 registry artifact without following redirects.
    pub async fn fetch_revision_tarball_response(
        &self,
        digest: &str,
    ) -> Result<FetchOutcome<ThrottledResponse>> {
        self.ensure_available()?;
        let url = format!("{}/-/tarballs/sha512/{digest}", self.base.trim_end_matches('/'));
        let client = self.client.acquire_for_url_without_redirects_with_priority(&url, 0).await;
        let started = Instant::now();
        let request = client.get(&url).timeout(self.timeout).headers(self.request_headers(&url));
        let response = self.run(request, &url).await?;
        if response.status() == StatusCode::NOT_FOUND {
            self.breaker.record_success();
            return Ok(FetchOutcome::NotFound);
        }
        let response = self.checked(response, &url).await?;
        self.breaker.record_success();
        Ok(FetchOutcome::Ok(
            client.retain_for_body(response, self.timeout.saturating_sub(started.elapsed())),
        ))
    }

    /// Query an upstream npm search endpoint with the caller's already-encoded
    /// query string. The upstream client contributes only configured headers,
    /// never headers supplied by the browser caller.
    pub async fn fetch_search(&self, query_string: &str) -> Result<FetchOutcome<SearchResponse>> {
        self.fetch_discovery_json(&format!("/-/v1/search?{query_string}")).await
    }

    /// Fetch the npm organization package map for one validated scope.
    pub async fn fetch_org_packages(
        &self,
        scope: &str,
    ) -> Result<FetchOutcome<Map<String, Value>>> {
        self.fetch_discovery_json(&format!("/-/org/{scope}/package")).await
    }

    async fn fetch_discovery_json<Payload: DeserializeOwned>(
        &self,
        path_and_query: &str,
    ) -> Result<FetchOutcome<Payload>> {
        self.ensure_available()?;
        let url = format!("{}{path_and_query}", self.base.trim_end_matches('/'));
        let client =
            self.client.acquire_for_url_without_redirects_with_priority(&url, UNPRIORITIZED).await;
        let request = client.get(&url).timeout(self.timeout).headers(self.request_headers(&url));
        let response = self.run(request, &url).await?;
        if response.status() == StatusCode::NOT_FOUND {
            self.breaker.record_success();
            return Ok(FetchOutcome::NotFound);
        }
        let response = self.checked(response, &url).await?;
        let body =
            read_limited_body(response, UPSTREAM_DISCOVERY_BODY_LIMIT).await.map_err(|err| {
                self.breaker.record_failure();
                RegistryError::UpstreamResponse { url: url.clone(), reason: err.to_string() }
            })?;
        if body.truncated {
            self.breaker.record_failure();
            return Err(RegistryError::UpstreamResponse {
                url,
                reason: format!(
                    "response body exceeds the {UPSTREAM_DISCOVERY_BODY_LIMIT}-byte limit",
                ),
            });
        }
        let parsed = serde_json::from_slice(&body.bytes).map_err(|err| {
            self.breaker.record_failure();
            RegistryError::UpstreamResponse { url, reason: err.to_string() }
        })?;
        self.breaker.record_success();
        Ok(FetchOutcome::Ok(parsed))
    }

    fn request_headers(&self, url: &str) -> HeaderMap {
        if same_origin(&self.base, url) && is_url_secure_for_credentials(url) {
            return self.headers.clone();
        }
        HeaderMap::new()
    }

    /// Fail fast with [`RegistryError::UpstreamUnavailable`] when the
    /// breaker is open, so callers never pay a request to a known-down
    /// upstream.
    fn ensure_available(&self) -> Result<()> {
        if self.breaker.try_acquire() {
            return Ok(());
        }
        Err(RegistryError::UpstreamUnavailable { upstream: self.name.clone() })
    }

    /// Send a built request, mapping a transport error to
    /// [`RegistryError::Upstream`] and counting it against the breaker.
    async fn run(&self, request: reqwest::RequestBuilder, url: &str) -> Result<reqwest::Response> {
        self.ensure_allowed_url(url)?;
        request.send().await.map_err(|source| {
            self.breaker.record_failure();
            RegistryError::Upstream { url: url.to_string(), source }
        })
    }

    async fn get_with_scoped_headers(
        &self,
        url: &str,
        headers: &HeaderMap,
    ) -> Result<(reqwest::Response, ThrottledClientGuard<'_>)> {
        self.ensure_allowed_url(url)?;
        let started = Instant::now();
        self.client
            .get_response_with_scoped_headers(url, |request, destination| {
                request
                    .timeout(self.timeout.saturating_sub(started.elapsed()))
                    .headers(self.request_headers(destination))
                    .headers(headers.clone())
            })
            .await
            .map_err(|source| {
                self.breaker.record_failure();
                RegistryError::Upstream { url: url.to_string(), source }
            })
    }

    fn ensure_allowed_url(&self, url: &str) -> Result<()> {
        if let Some(guard) = &self.fetch_guard
            && !reqwest::Url::parse(url).is_ok_and(|url| guard(&url))
        {
            return Err(RegistryError::UpstreamResponse {
                url: url.to_string(),
                reason: "URL is not allowed by the fetch allowlist".to_string(),
            });
        }
        Ok(())
    }

    /// Pass a successful response through; map any non-success status to
    /// [`RegistryError::UpstreamStatus`]. Only a `5xx` counts against the
    /// breaker — a non-404 `4xx` is an authoritative client error (auth,
    /// rate-limit, bad request), not an availability signal, so it leaves
    /// the breaker untouched rather than opening the circuit and masking
    /// the real status behind a `503`. The `404`/`304` paths are handled
    /// by the callers before reaching here.
    async fn checked(&self, response: reqwest::Response, url: &str) -> Result<reqwest::Response> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        if status.is_server_error() {
            self.breaker.record_failure();
        }
        let body = read_upstream_error_body(response).await;
        Err(RegistryError::UpstreamStatus { url: url.to_string(), status: status.as_u16(), body })
    }
}

async fn read_upstream_error_body(response: reqwest::Response) -> String {
    let Ok(body) = read_limited_body(response, UPSTREAM_ERROR_BODY_LIMIT).await else {
        return String::new();
    };
    let mut text = String::from_utf8_lossy(&body.bytes).into_owned();
    if body.truncated {
        if !text.is_empty() && !text.chars().next_back().is_some_and(char::is_whitespace) {
            text.push(' ');
        }
        text.push_str("(response body truncated)");
    }
    text
}

#[cfg(test)]
mod tests;
