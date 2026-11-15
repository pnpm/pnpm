use std::time::Duration;

use axum::{
    Router,
    extract::Request,
    http::{Method, StatusCode},
    middleware::{self, Next},
    response::Response,
};
use tokio::time::Instant;

use crate::server::AppState;

use super::EnabledSurfaces;

#[derive(Clone, Copy)]
pub(crate) struct RequestDeadline(pub(crate) Instant);

pub(super) fn apply_request_timeout(
    router: Router<AppState>,
    surfaces: EnabledSurfaces,
    timeout_duration: Option<Duration>,
) -> Router<AppState> {
    let Some(timeout_duration) = timeout_duration else {
        return router;
    };
    router.layer(middleware::from_fn(move |mut request: Request, next: Next| async move {
        let is_resolver = is_resolver_request(&request, surfaces);
        let deadline = Instant::now() + timeout_duration;
        request
            .extensions_mut()
            .insert(RequestDeadline(deadline));
        match tokio::time::timeout_at(deadline, next.run(request)).await {
            Ok(response) => response,
            Err(_) if is_resolver => crate::resolver::resolve_timeout_response(),
            Err(_) => request_timeout_response(),
        }
    }))
}

fn is_resolver_request(request: &Request, surfaces: EnabledSurfaces) -> bool {
    surfaces.resolver
        && request.method() == Method::POST
        && matches!(request.uri().path(), "/-/pnpr/v0/resolve" | "/-/pnpr/v0/verify-lockfile")
}

fn request_timeout_response() -> Response {
    pnpr_error::json_error(
        StatusCode::GATEWAY_TIMEOUT,
        "request timed out",
        Some("ERR_PNPR_REQUEST_TIMEOUT"),
    )
}
