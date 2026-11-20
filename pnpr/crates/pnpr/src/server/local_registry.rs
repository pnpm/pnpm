use super::{
    AppInner,
    AppState,
    AuthedCaller,
    RoutingSnapshot,
    routing::local_registry_router,
};
use axum::{
    Router,
    body::Body,
    http::Request,
};
use pnpr_policy::Identity;
use pnpr_route::LocalFetch;
use reqwest::ResponseBuilderExt;
use std::sync::{
    Arc,
    Weak,
};
use tower::ServiceExt;
use url::Url;

pub(crate) struct LocalRegistry {
    pub(crate) public_url: String,
    graph_generation: Option<u64>,
    state: Weak<AppInner>,
    router: Router<AppState>,
}

impl LocalRegistry {
    pub(crate) fn new(public_url: String) -> Self {
        Self { public_url, graph_generation: None, state: Weak::new(), router: Router::new() }
    }

    pub(crate) fn attach(&mut self, state: &AppState, routing: &RoutingSnapshot) {
        self.graph_generation = routing.generation;
        self.state = Arc::downgrade(&state.inner);
        self.router = local_registry_router(&state.inner.config, routing);
    }

    /// Stored `(graph, grants)` revision stamp for the lockfile verdict cache,
    /// or `None` while a grant reload is applying new rules and invalidating
    /// caches so concurrent verifications bypass cached verdicts across the
    /// transition.
    pub(crate) fn verdict_epoch(&self) -> Option<String> {
        let grants_generation = match self.state.upgrade() {
            Some(inner) => inner.identity.managed.grants.stable_generation()?,
            None => None,
        };
        Some(match (self.graph_generation, grants_generation) {
            (None, None) => String::new(),
            (graph, grants) => {
                let graph = graph.map(|value| value.to_string()).unwrap_or_default();
                let grants = grants.map(|value| value.to_string()).unwrap_or_default();
                format!("{graph}:{grants}")
            }
        })
    }

    pub(crate) fn fetcher(&self, identity: Identity) -> Arc<LocalFetch> {
        let base = Url::parse(&format!("{}/", self.public_url.trim_end_matches('/'))).ok();
        let router = self.state
            .upgrade()
            .map(|inner| {
                self.router
                    .clone()
                    .with_state(AppState { inner })
            })
            .unwrap_or_default();
        Arc::new(move |url, headers| {
            let url = Url::parse(url).ok()?;
            let base = base.as_ref()?;
            let uri = local_uri(base, &url)?;
            let router = router.clone();
            let mut request = Request::get(uri)
                .body(Body::empty())
                .expect("parsed URL path");
            *request.headers_mut() = headers;
            request
                .extensions_mut()
                .insert(AuthedCaller(identity.clone()));
            Some(Box::pin(serve(router, request, url)))
        })
    }
}

fn local_uri(base: &Url, url: &Url) -> Option<String> {
    if url.origin() != base.origin() {
        return None;
    }
    let path = url.path().strip_prefix(base.path())?;
    let mut uri = format!("/{path}");
    if let Some(query) = url.query() {
        uri.push('?');
        uri.push_str(query);
    }
    Some(uri)
}

async fn serve(router: Router, request: Request<Body>, url: Url) -> reqwest::Response {
    let response = router.oneshot(request).await.expect("infallible registry router");
    let (parts, body) = response.into_parts();
    let mut builder = axum::http::Response::builder().status(parts.status).url(url);
    *builder.headers_mut().expect("valid response") = parts.headers;
    builder
        .body(reqwest::Body::wrap_stream(body.into_data_stream()))
        .expect("valid response")
        .into()
}
