use super::{Arc, Duration, HeaderMap, RedirectGuard, ThrottledClient, UpstreamConfig};

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
