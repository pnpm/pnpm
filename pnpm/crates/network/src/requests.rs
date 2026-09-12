use super::{
    AuthHeaders, MAX_REDIRECT_HOPS, RetryOpts, SecureAuthResponse, ThrottledClient,
    ThrottledClientGuard, UNPRIORITIZED, is_redirect_status, read_limited_body, retry,
};

impl ThrottledClient {
    /// Send a GET whose URL-scoped credentials are re-evaluated for every
    /// redirect target. Credentials are limited to TLS and loopback URLs by
    /// [`AuthHeaders::for_secure_url`].
    pub async fn get_bytes_with_secure_auth_headers(
        &self,
        url: &str,
        auth_headers: &AuthHeaders,
    ) -> Result<SecureAuthResponse, reqwest::Error> {
        self.get_bytes_with_secure_auth_and_accept(url, auth_headers, None).await
    }

    /// Retry a complete authenticated GET, including redirects and body reads.
    /// Each attempt re-evaluates credentials and releases permits before backoff.
    pub async fn get_bytes_with_secure_auth_and_retry(
        &self,
        url: &str,
        auth_headers: &AuthHeaders,
        accept: Option<&str>,
        retry_opts: RetryOpts,
    ) -> Result<SecureAuthResponse, reqwest::Error> {
        self.get_limited_bytes_with_secure_auth_and_retry(
            url,
            auth_headers,
            accept,
            retry_opts,
            usize::MAX,
        )
        .await
    }

    /// An authenticated GET with a byte limit on the final response, including
    /// error responses. Oversized bodies are marked by [`SecureAuthResponse::body_truncated`]
    /// and are not retried.
    pub async fn get_limited_bytes_with_secure_auth_and_retry(
        &self,
        url: &str,
        auth_headers: &AuthHeaders,
        accept: Option<&str>,
        retry_opts: RetryOpts,
        body_limit: usize,
    ) -> Result<SecureAuthResponse, reqwest::Error> {
        retry::get_secure_bytes(self, url, auth_headers, accept, retry_opts, body_limit).await
    }

    /// Negotiate an ecosystem's metadata representation while retaining the
    /// shared request budget and URL-scoped authorization on redirects.
    pub async fn get_bytes_with_secure_auth_and_accept(
        &self,
        url: &str,
        auth_headers: &AuthHeaders,
        accept: Option<&str>,
    ) -> Result<SecureAuthResponse, reqwest::Error> {
        self.get_limited_bytes_with_secure_auth_and_accept(url, auth_headers, accept, usize::MAX)
            .await
    }

    pub(super) async fn get_limited_bytes_with_secure_auth_and_accept(
        &self,
        url: &str,
        auth_headers: &AuthHeaders,
        accept: Option<&str>,
        body_limit: usize,
    ) -> Result<SecureAuthResponse, reqwest::Error> {
        let (response, _guard) = self
            .get_response_with_scoped_headers(url, |mut request, url| {
                if let Some(accept) = accept {
                    request = request.header(reqwest::header::ACCEPT, accept);
                }
                if let Some(authorization) = auth_headers.for_secure_url(url) {
                    request = request.header("authorization", authorization);
                }
                request
            })
            .await?;
        let status = response.status();
        let url = response.url().to_string();
        let body = read_limited_body(response, body_limit).await?;
        Ok(SecureAuthResponse { status, body: body.bytes, body_truncated: body.truncated, url })
    }

    /// Follow a GET's redirects, rebuilding its headers for each destination.
    /// The returned guard retains the request budget while the caller reads
    /// the response body. Configured redirect guards apply at every hop.
    pub async fn get_response_with_scoped_headers(
        &self,
        url: &str,
        configure: impl Fn(reqwest::RequestBuilder, &str) -> reqwest::RequestBuilder,
    ) -> Result<(reqwest::Response, ThrottledClientGuard<'_>), reqwest::Error> {
        let mut current_url = url.to_string();
        for redirect_count in 0..=MAX_REDIRECT_HOPS {
            let client = self
                .acquire_for_url_without_redirects_with_priority(&current_url, UNPRIORITIZED)
                .await;
            let request = configure(client.get(&current_url), &current_url);
            let response = request.send().await?;
            let target = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|location| location.to_str().ok())
                .and_then(|location| response.url().join(location).ok());
            if is_redirect_status(response.status())
                && redirect_count < MAX_REDIRECT_HOPS
                && let Some(target) = target
            {
                current_url = target.to_string();
                continue;
            }
            return Ok((response, client));
        }
        unreachable!()
    }

    pub(super) async fn acquire_for_url_with_priority_and_redirects(
        &self,
        url: &str,
        priority: u64,
        follow_redirects: bool,
    ) -> ThrottledClientGuard<'_> {
        // Acquire the per-origin `maxSockets` permit *before* the global
        // concurrency permit: a request queued behind a saturated origin must
        // not hold a global slot while it waits, or a burst to one origin would
        // hoard every global permit and starve requests to other origins.
        let host_permit = match &self.host_socket_limit {
            Some(limit) => limit.acquire(url).await,
            None => None,
        };
        let permit = self.semaphore.acquire(priority).await;
        let clients = self.per_registry.pick_value_for_url(url).unwrap_or(&self.default_clients);
        let client = clients.select(follow_redirects);
        ThrottledClientGuard { permit, host_permit, client }
    }
}
