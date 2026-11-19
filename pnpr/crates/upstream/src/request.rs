//! Sending a request to an upstream and checking its response status.

use super::{
    CanonicalPackageName, HeadError, HeaderMap, Instant, RedirectGuard, RegistryError, Result,
    ThrottledClient, ThrottledClientGuard, Upstream,
    http::{diagnostic_url, read_upstream_error_body},
    version_route_tarball_version,
};

/// Where an upstream packument's `dist.tarball` points for a version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpstreamTarballRoute {
    /// A relative `pnpr` `{pkg}@{version}` route on the upstream's base URL.
    Version(String),
    /// An absolute HTTP(S) `dist.tarball` URL (same-origin or split-domain CDN).
    Url(String),
    /// A relative or missing `dist.tarball`, fetched at `{base}/{pkg}/-/{filename}`.
    Archive,
}

impl UpstreamTarballRoute {
    #[must_use]
    pub fn from_dist_tarball(url: Option<&str>, pkg: &CanonicalPackageName) -> Self {
        let Some(url) = url else {
            return Self::Archive;
        };
        if is_http_tarball_url(url) {
            return Self::Url(url.to_string());
        }
        if !url.contains(['?', '#'])
            && let Some(version) = version_route_tarball_version(url, pkg)
        {
            return Self::Version(version.to_string());
        }
        Self::Archive
    }
}

fn is_http_tarball_url(url: &str) -> bool {
    let bytes = url.as_bytes();
    bytes
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"https://"))
        || bytes
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"http://"))
}

impl Upstream {
    /// Send a built request, mapping a transport error to
    /// [`RegistryError::Upstream`] and counting it against the breaker.
    pub(crate) async fn run(
        &self,
        request: reqwest::RequestBuilder,
        url: &str,
    ) -> Result<reqwest::Response> {
        self.ensure_allowed_url(url)?;
        request
            .send()
            .await
            .map_err(|source| {
                self.breaker.record_failure();
                RegistryError::Upstream { url: diagnostic_url(url), source: source.without_url() }
            })
    }

    /// Body timeouts can reflect local scheduling delays after a healthy response head.
    pub(crate) fn record_body_failure(&self, error: &reqwest::Error) {
        if !error.is_timeout() {
            self.breaker.record_failure();
        }
    }

    pub(crate) async fn get_with_scoped_headers(
        &self,
        url: &str,
        headers: &HeaderMap,
        deadline: Deadline,
    ) -> Result<(reqwest::Response, ThrottledClientGuard<'_>)> {
        self.get_using_client(
            &self.http.client,
            self.http.fetch_guard.as_ref(),
            url,
            headers,
            deadline,
        )
        .await
    }

    pub(crate) async fn get_tarball_with_scoped_headers(
        &self,
        url: &str,
    ) -> Result<(reqwest::Response, ThrottledClientGuard<'_>)> {
        let client = self.http.declared_tarball_client.as_ref().unwrap_or(&self.http.client);
        let guard =
            self.http.declared_tarball_guard.as_ref().or(self.http.fetch_guard.as_ref());
        self.get_using_client(client, guard, url, &HeaderMap::new(), Deadline::Head).await
    }

    async fn get_using_client<'a>(
        &'a self,
        client: &'a ThrottledClient,
        guard: Option<&RedirectGuard>,
        url: &str,
        headers: &HeaderMap,
        deadline: Deadline,
    ) -> Result<(reqwest::Response, ThrottledClientGuard<'a>)> {
        self.ensure_url_passes_guard(url, guard)?;
        let started = Instant::now();
        let configure = |request: reqwest::RequestBuilder, destination: &str| {
            let request = request
                .headers(self.request_headers(destination))
                .headers(headers.clone());
            match deadline {
                Deadline::Whole => {
                    request.timeout(self.http.timeout.saturating_sub(started.elapsed()))
                }
                Deadline::Head => request,
            }
        };
        let response = match deadline {
            Deadline::Whole => client
                .get_response_with_scoped_headers(url, configure)
                .await
                .map_err(HeadError::Request),
            Deadline::Head => {
                client.get_response_head_within(url, self.http.timeout, configure).await
            }
        };
        response.map_err(|error| self.map_head_error(url, error))
    }

    fn map_head_error(&self, url: &str, error: HeadError) -> RegistryError {
        self.breaker.record_failure();
        let url = diagnostic_url(url);
        match error {
            HeadError::Request(source) => {
                RegistryError::Upstream { url, source: source.without_url() }
            }
            HeadError::TimedOut => RegistryError::UpstreamTimeout { url },
        }
    }

    /// Wait for `response` until `deadline`. Used for a body pnpr streams:
    /// the head is bounded
    /// here and the body by [`ThrottledResponse::bytes_stream`](pnpm_network::ThrottledResponse::bytes_stream).
    pub(crate) async fn within_head_deadline<Response>(
        &self,
        url: &str,
        deadline: Instant,
        response: impl Future<Output = Result<Response>>,
    ) -> Result<Response> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        tokio::time::timeout(remaining, response).await
            .unwrap_or_else(|_| {
                self.breaker.record_failure();
                Err(RegistryError::UpstreamTimeout { url: diagnostic_url(url) })
            })
    }

    pub(crate) fn ensure_allowed_url(&self, url: &str) -> Result<()> {
        self.ensure_url_passes_guard(url, self.http.fetch_guard.as_ref())
    }

    fn ensure_url_passes_guard(&self, url: &str, guard: Option<&RedirectGuard>) -> Result<()> {
        self.ensure_may_fetch()?;
        if let Some(guard) = guard
            && !reqwest::Url::parse(url).is_ok_and(|url| guard(&url))
        {
            return Err(RegistryError::UpstreamResponse {
                url: diagnostic_url(url),
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
    pub(crate) async fn checked(
        &self,
        response: reqwest::Response,
        url: &str,
    ) -> Result<reqwest::Response> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        if status.is_server_error() {
            self.breaker.record_failure();
        }
        // A streamed request has no deadline on its body, so the error body
        // gets the upstream timeout here; one that does not arrive in time is
        // reported as empty.
        let body = tokio::time::timeout(self.http.timeout, read_upstream_error_body(response))
            .await
            .unwrap_or_default();
        Err(RegistryError::UpstreamStatus {
            url: diagnostic_url(url),
            status: status.as_u16(),
            body,
        })
    }
}

/// What an upstream request's `timeout` bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Deadline {
    /// The whole response, for a body pnpr reads into memory.
    Whole,
    /// The wait for the response head, for a body pnpr streams. The body is
    /// bounded per chunk instead, so a download that keeps arriving is not
    /// cut off.
    Head,
}
