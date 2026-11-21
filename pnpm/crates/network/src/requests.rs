use super::{
    AuthHeaders,
    Client,
    ClientSet,
    Duration,
    Instant,
    MAX_REDIRECT_HOPS,
    RedirectGuard,
    RetryOpts,
    SecureAuthResponse,
    ThrottledClient,
    ThrottledClientGuard,
    UNPRIORITIZED,
    is_redirect_status,
    is_url_secure_for_credentials,
    read_limited_body,
    retry,
};

impl ThrottledClient {
    async fn read_response(
        &self,
        response: reqwest::Response,
        body_limit: usize,
    ) -> Result<SecureAuthResponse, reqwest::Error> {
        let status = response.status();
        let url = response.url().to_string();
        let body = read_limited_body(response, body_limit).await
            .inspect_err(|error| self.downscale_on_timeout(&url, error))?;
        Ok(SecureAuthResponse { status, body: body.bytes, body_truncated: body.truncated, url })
    }

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

    /// Retry a complete GET like [`Self::get_limited_bytes_with_secure_auth_and_retry`],
    /// with credentials from [`AuthHeaders::for_url`] at every hop, so plain
    /// HTTP registries receive their credentials too. A request that starts on
    /// a URL [`is_url_secure_for_credentials`] accepts sends no credentials to
    /// a hop it rejects.
    pub async fn get_limited_bytes_with_auth_and_retry(
        &self,
        url: &str,
        auth_headers: &AuthHeaders,
        accept: Option<&str>,
        retry_opts: RetryOpts,
        body_limit: usize,
    ) -> Result<SecureAuthResponse, reqwest::Error> {
        let authorize = authorize_without_downgrade(auth_headers, url);
        retry::get_bytes(url, retry_opts, || {
            self.get_limited_bytes_with_scoped_auth(
                url,
                |request, hop| crate::auth::apply_authorization(request, authorize(hop)),
                accept,
                body_limit,
            )
        })
        .await
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
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(value) = accept.map(str::parse).and_then(Result::ok) {
            headers.insert(reqwest::header::ACCEPT, value);
        }
        if let Some(response) = auth_headers.fetch_local(url, None, headers) {
            return self.read_response(response.await, body_limit).await;
        }
        let authorize = auth_headers.secure_request_auth(url);
        self.get_limited_bytes_with_scoped_auth(url, authorize, accept, body_limit).await
    }

    /// Follow a GET's redirects, attaching `authorize`'s header for each hop's URL.
    pub(super) async fn get_limited_bytes_with_scoped_auth(
        &self,
        url: &str,
        authorize: impl Fn(reqwest::RequestBuilder, &str) -> reqwest::RequestBuilder + Sync,
        accept: Option<&str>,
        body_limit: usize,
    ) -> Result<SecureAuthResponse, reqwest::Error> {
        let (response, _guard) = self.get_response_with_scoped_headers(url, |mut request, url| {
            if let Some(accept) = accept {
                request = request.header(reqwest::header::ACCEPT, accept);
            }
            authorize(request, url)
        })
        .await?;
        self.read_response(response, body_limit).await
    }

    /// Follow a GET's redirects, rebuilding its headers for each destination.
    /// The returned guard retains the request budget while the caller reads
    /// the response body. Configured redirect guards apply at every hop.
    pub async fn get_response_with_scoped_headers(
        &self,
        url: &str,
        configure: impl Fn(reqwest::RequestBuilder, &str) -> reqwest::RequestBuilder,
    ) -> Result<(reqwest::Response, ThrottledClientGuard<'_>), reqwest::Error> {
        self.package_response_with_scoped_headers(
            reqwest::Method::GET,
            url,
            UNPRIORITIZED,
            configure,
        )
        .await
    }

    /// Like [`Self::get_response_with_scoped_headers`], validating the initial
    /// URL and every redirect destination against `guard` before sending.
    pub async fn get_response_with_scoped_headers_guarded(
        &self,
        url: &str,
        guard: Option<&RedirectGuard>,
        configure: impl Fn(reqwest::RequestBuilder, &str) -> reqwest::RequestBuilder,
    ) -> Result<(reqwest::Response, ThrottledClientGuard<'_>), HeadError> {
        self.response_with_scoped_headers(
            reqwest::Method::GET,
            url,
            None,
            guard,
            UNPRIORITIZED,
            configure,
        )
        .await
    }

    /// Like [`Self::get_response_with_scoped_headers`], failing with
    /// [`HeadError::TimedOut`] when the response head has not arrived within
    /// `timeout`. Only the time each hop spends after taking its permits
    /// counts, so queueing for them does not, and redirects share the rest of
    /// `timeout`. The response body has no deadline here.
    pub async fn get_response_head_within(
        &self,
        url: &str,
        timeout: Duration,
        configure: impl Fn(reqwest::RequestBuilder, &str) -> reqwest::RequestBuilder,
    ) -> Result<(reqwest::Response, ThrottledClientGuard<'_>), HeadError> {
        self.response_with_scoped_headers(
            reqwest::Method::GET,
            url,
            Some(timeout),
            None,
            UNPRIORITIZED,
            configure,
        )
        .await
    }

    /// Like [`Self::get_response_head_within`], validating the initial URL and
    /// every redirect destination against `guard` before sending.
    pub async fn get_response_head_within_guarded(
        &self,
        url: &str,
        timeout: Duration,
        guard: Option<&RedirectGuard>,
        configure: impl Fn(reqwest::RequestBuilder, &str) -> reqwest::RequestBuilder,
    ) -> Result<(reqwest::Response, ThrottledClientGuard<'_>), HeadError> {
        self.response_with_scoped_headers(
            reqwest::Method::GET,
            url,
            Some(timeout),
            guard,
            UNPRIORITIZED,
            configure,
        )
        .await
    }

    /// Apply the per-redirect header configuration and request-budget guarantees
    /// documented by [`Self::get_response_with_scoped_headers`].
    pub async fn head_response_with_scoped_headers(
        &self,
        url: &str,
        configure: impl Fn(reqwest::RequestBuilder, &str) -> reqwest::RequestBuilder,
    ) -> Result<(reqwest::Response, ThrottledClientGuard<'_>), reqwest::Error> {
        self.package_response_with_scoped_headers(
            reqwest::Method::HEAD,
            url,
            UNPRIORITIZED,
            configure,
        )
        .await
    }

    pub(super) async fn package_response_with_scoped_headers(
        &self,
        method: reqwest::Method,
        url: &str,
        priority: u64,
        configure: impl Fn(reqwest::RequestBuilder, &str) -> reqwest::RequestBuilder,
    ) -> Result<(reqwest::Response, ThrottledClientGuard<'_>), reqwest::Error> {
        self.response_with_scoped_headers(method, url, None, None, priority, configure)
            .await
            .map_err(HeadError::into_request_error)
    }

    async fn response_with_scoped_headers(
        &self,
        method: reqwest::Method,
        url: &str,
        head_timeout: Option<Duration>,
        guard: Option<&RedirectGuard>,
        priority: u64,
        configure: impl Fn(reqwest::RequestBuilder, &str) -> reqwest::RequestBuilder,
    ) -> Result<(reqwest::Response, ThrottledClientGuard<'_>), HeadError> {
        let mut current_url = url.to_string();
        let mut head_budget = head_timeout;
        for redirect_count in 0..=MAX_REDIRECT_HOPS {
            ensure_hop_url_allowed(&current_url, guard)?;
            let client =
                self.acquire_for_url_without_redirects_with_priority(&current_url, priority).await;
            let hop_started = client.acquired_at();
            let deadline = head_budget.map(|budget| hop_started + budget);
            let request = configure(client.request(method.clone(), &current_url), &current_url);
            // A head timeout lowers the origin's cap the way a request timeout
            // does, while this hop still holds its permit.
            let response = within(deadline, request.send()).await
                .inspect_err(|_| {
                    self.downscale_while_peers_active(&current_url);
                })?
                .inspect_err(|error| self.downscale_on_timeout(&current_url, error))
                .map_err(classify_hop_error)?;
            if let Some(target) = next_redirect_target(&response, redirect_count) {
                ensure_target_allowed(&target, guard)?;
                head_budget =
                    head_budget.map(|budget| budget.saturating_sub(hop_started.elapsed()));
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
        self.acquire_with_client(url, priority, |clients| clients.select(follow_redirects, true))
            .await
    }

    pub(super) async fn acquire_with_client(
        &self,
        url: &str,
        priority: u64,
        select: impl FnOnce(&ClientSet) -> &Client,
    ) -> ThrottledClientGuard<'_> {
        // Acquire the per-origin permits *before* the global
        // concurrency permit: a request queued behind a saturated origin must
        // not hold a global slot while it waits, or a burst to one origin would
        // hoard every global permit and starve requests to other origins.
        let (mut origin_permit, host_permit) = match self.proxy_routing.effective_socket_origin(url)
        {
            Some((origin, is_proxied)) => {
                let (origin_permit, host_permit) =
                    self.origin_limits.acquire(url, &origin, is_proxied, priority).await;
                (Some(origin_permit), host_permit)
            }
            None => (None, None),
        };
        let permit = self.semaphore.acquire(priority).await;
        if let Some(origin_permit) = &mut origin_permit {
            origin_permit.mark_active();
        }
        let clients = self.per_registry.pick_value_for_url(url).unwrap_or(&self.default_clients);
        let client = select(clients);
        ThrottledClientGuard {
            permit,
            host_permit,
            origin_permit,
            client,
            acquired_at: Instant::now(),
        }
    }
}

pub(super) fn authorize_without_downgrade<'auth>(
    auth_headers: &'auth AuthHeaders,
    start_url: &str,
) -> impl Fn(&str) -> Option<String> + Copy + Sync + 'auth {
    let starts_secure = is_url_secure_for_credentials(start_url);
    move |url: &str| {
        if starts_secure && !is_url_secure_for_credentials(url) {
            return None;
        }
        auth_headers.for_url(url)
    }
}

/// Why [`ThrottledClient::get_response_head_within`] returned no response.
#[derive(Debug, derive_more::Display, derive_more::Error)]
pub enum HeadError {
    Request(reqwest::Error),
    /// A request or redirect destination was rejected by the fetch allowlist.
    #[display("URL is not allowed by the fetch allowlist")]
    BlockedUrl(#[error(ignore)] Option<reqwest::Error>),
    /// The response head did not arrive in time.
    #[display("the response head did not arrive in time")]
    TimedOut,
}

impl HeadError {
    /// The request error of a call that set no head deadline or custom guard.
    fn into_request_error(self) -> reqwest::Error {
        match self {
            Self::Request(error) | Self::BlockedUrl(Some(error)) => error,
            Self::BlockedUrl(None) => {
                unreachable!("an unguarded request surfaces reqwest's own redirect error")
            }
            Self::TimedOut => unreachable!("a request without a head deadline cannot time out"),
        }
    }
}

fn ensure_hop_url_allowed(url: &str, guard: Option<&RedirectGuard>) -> Result<(), HeadError> {
    if let Some(guard) = guard
        && !reqwest::Url::parse(url).is_ok_and(|url| guard(&url))
    {
        return Err(HeadError::BlockedUrl(None));
    }
    Ok(())
}

fn ensure_target_allowed(
    target: &reqwest::Url,
    guard: Option<&RedirectGuard>,
) -> Result<(), HeadError> {
    if let Some(guard) = guard
        && !guard(target)
    {
        return Err(HeadError::BlockedUrl(None));
    }
    Ok(())
}

fn classify_hop_error(error: reqwest::Error) -> HeadError {
    if crate::redirect_guard::is_blocked_redirect(&error) {
        HeadError::BlockedUrl(Some(error))
    } else {
        HeadError::Request(error)
    }
}

fn next_redirect_target(
    response: &reqwest::Response,
    redirect_count: usize,
) -> Option<reqwest::Url> {
    if !is_redirect_status(response.status()) || redirect_count >= MAX_REDIRECT_HOPS {
        return None;
    }
    response
        .headers()
        .get(reqwest::header::LOCATION)
        .map(reqwest::header::HeaderValue::to_str)
        .and_then(Result::ok)
        .and_then(|location| response.url().join(location).ok())
}

/// Await `future`, failing with [`HeadError::TimedOut`] at `deadline`.
async fn within<Output>(
    deadline: Option<Instant>,
    future: impl Future<Output = Output>,
) -> Result<Output, HeadError> {
    let Some(deadline) = deadline else { return Ok(future.await) };
    tokio::time::timeout_at(deadline.into(), future).await.map_err(|_| HeadError::TimedOut)
}
