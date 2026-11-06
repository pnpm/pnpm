//! Sending a request to an upstream and checking its response status.

use super::{
    HeadError, HeaderMap, Instant, RegistryError, Result, ThrottledClientGuard, Upstream,
    http::read_upstream_error_body,
};

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
                RegistryError::Upstream { url: url.to_string(), source }
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
        self.ensure_allowed_url(url)?;
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
            Deadline::Whole => self.http.client
                .get_response_with_scoped_headers(url, configure)
                .await
                .map_err(HeadError::Request),
            Deadline::Head => {
                self.http.client.get_response_head_within(url, self.http.timeout, configure).await
            }
        };
        response.map_err(|error| {
            self.breaker.record_failure();
            match error {
                HeadError::Request(source) => {
                    RegistryError::Upstream { url: url.to_string(), source }
                }
                HeadError::TimedOut => RegistryError::UpstreamTimeout { url: url.to_string() },
            }
        })
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
                Err(RegistryError::UpstreamTimeout { url: url.to_string() })
            })
    }

    pub(crate) fn ensure_allowed_url(&self, url: &str) -> Result<()> {
        if let Some(guard) = &self.http.fetch_guard
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
        Err(RegistryError::UpstreamStatus { url: url.to_string(), status: status.as_u16(), body })
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
