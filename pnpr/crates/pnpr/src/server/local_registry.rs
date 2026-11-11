use super::{AppInner, AppState, AuthedCaller, routing::local_registry_router};
use axum::{Router, body::Body, http::Request};
use pnpr_policy::Identity;
use pnpr_route::LocalFetch;
use reqwest::ResponseBuilderExt;
use std::sync::{Arc, OnceLock, Weak};
use tower::ServiceExt;
use url::Url;

pub(crate) struct LocalRegistry {
    pub(crate) public_url: String,
    state: Weak<AppInner>,
}

impl LocalRegistry {
    pub(crate) fn new(public_url: String) -> Self {
        Self { public_url, state: Weak::new() }
    }

    pub(crate) fn attach(&mut self, state: &AppState) {
        self.state = Arc::downgrade(&state.inner);
    }

    pub(crate) fn fetcher(&self, identity: Identity) -> Arc<LocalFetch> {
        let base = Url::parse(&format!("{}/", self.public_url.trim_end_matches('/'))).ok();
        let state = self.state.upgrade();
        let router = OnceLock::new();
        Arc::new(move |url, headers| {
            let url = Url::parse(url).ok()?;
            let base = base.as_ref()?;
            let uri = local_uri(base, &url)?;
            let router = router
                .get_or_init(|| {
                    state
                        .as_ref()
                        .map(|inner| local_registry_router(AppState { inner: Arc::clone(inner) }))
                        .unwrap_or_default()
                })
                .clone();
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
