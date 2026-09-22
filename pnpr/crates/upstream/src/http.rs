use super::{
    Arc,
    Duration,
    HeaderMap,
    RedirectGuard,
    ThrottledClient,
    UpstreamConfig,
};

#[derive(Clone)]
pub(super) struct UpstreamHttp {
    pub(super) client: Arc<ThrottledClient>,
    pub(super) fetch_guard: Option<RedirectGuard>,
    /// Resolved per-upstream request headers (auth + custom) attached to
    /// every fetch. Empty for an upstream with no `auth:`/`headers:`.
    pub(super) headers: HeaderMap,
    /// Per-request deadline (verdaccio's `timeout`).
    pub(super) timeout: Duration,
}

impl UpstreamHttp {
    pub(super) fn new(config: &UpstreamConfig) -> Self {
        Self {
            client: Arc::new(ThrottledClient::new_for_installs()),
            fetch_guard: None,
            headers: config.headers.clone(),
            timeout: config.requests.timeout,
        }
    }
}

pub(super) const UPSTREAM_ERROR_BODY_LIMIT: usize = 64 * 1024;

pub(super) async fn read_upstream_error_body(response: reqwest::Response) -> String {
    let Ok(body) = pnpm_network::read_limited_body(response, UPSTREAM_ERROR_BODY_LIMIT).await
    else {
        return String::new();
    };
    let mut text = String::from_utf8_lossy(&body.bytes).into_owned();
    if body.truncated {
        if !text.is_empty()
            && !text
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace)
        {
            text.push(' ');
        }
        text.push_str("(response body truncated)");
    }
    text
}

/// Whether two URLs share a scheme, host and port, so a credential meant for
/// one may be sent to the other.
pub(super) fn same_origin(base: &str, url: &str) -> bool {
    let (Ok(base), Ok(url)) = (reqwest::Url::parse(base), reqwest::Url::parse(url)) else {
        return false;
    };
    base.scheme() == url.scheme()
        && base
            .host_str()
            .is_some_and(|host| Some(host) == url.host_str())
        && base.port_or_known_default() == url.port_or_known_default()
}
