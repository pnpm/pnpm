use super::{
    Arc, Duration, HeaderMap, RedirectGuard, ThrottledClient, UPSTREAM_ERROR_BODY_LIMIT,
    UpstreamConfig, read_limited_body,
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

pub(super) async fn read_upstream_error_body(response: reqwest::Response) -> String {
    let Ok(body) = read_limited_body(response, UPSTREAM_ERROR_BODY_LIMIT).await else {
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
