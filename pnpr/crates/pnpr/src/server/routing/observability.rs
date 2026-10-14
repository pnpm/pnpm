use std::{sync::Arc, time::Duration};

use axum::{Router, body::Body, extract::ConnectInfo, http::Request, response::Response};
use pnpr_config::TrustedProxyHeader;
use tower_http::{
    compression::{
        CompressionLayer,
        predicate::{DefaultPredicate, NotForContentType, Predicate as _},
    },
    trace::TraceLayer,
};
use tracing::Span;

use crate::server::{AppState, PeerAddr, loggable_uri, resolve_client_ip};

pub(super) fn with_observability_layers(
    router: Router<AppState>,
    trusted_proxies: Arc<[pnpr_config::IpNetwork]>,
    trusted_proxy_header: TrustedProxyHeader,
) -> Router<AppState> {
    let router = with_compression(router);
    with_access_trace(router, trusted_proxies, trusted_proxy_header)
}

fn with_compression(router: Router<AppState>) -> Router<AppState> {
    router.layer(
        CompressionLayer::new()
            .compress_when(
                DefaultPredicate::new()
                    .and(NotForContentType::const_new("application/octet-stream"))
                    .and(NotForContentType::const_new("application/x-ndjson")),
            ),
    )
}

fn with_access_trace(
    router: Router<AppState>,
    trusted_proxies: Arc<[pnpr_config::IpNetwork]>,
    trusted_proxy_header: TrustedProxyHeader,
) -> Router<AppState> {
    router.layer(
        TraceLayer::new_for_http()
            .make_span_with(move |request: &Request<Body>| {
                let peer = request
                    .extensions()
                    .get::<ConnectInfo<PeerAddr>>()
                    .map(|ConnectInfo(PeerAddr(addr))| addr.ip());
                let client_ip = resolve_client_ip(
                    peer,
                    request.headers(),
                    &trusted_proxies,
                    trusted_proxy_header,
                );
                let span = tracing::info_span!(
                    target: "pnpr::access",
                    "request",
                    method = %request.method(),
                    uri = %loggable_uri(request.uri()),
                    client_ip = tracing::field::Empty,
                    cache = tracing::field::Empty,
                );
                if let Some(ip) = client_ip.or(peer) {
                    span.record("client_ip", tracing::field::display(ip));
                }
                span
            })
            .on_request(())
            .on_response(|response: &Response<Body>, latency: Duration, _span: &Span| {
                tracing::info!(
                    target: "pnpr::access",
                    status = response.status().as_u16(),
                    latency_ms = latency.as_millis() as u64,
                    "finished processing request",
                );
            })
            .on_failure(()),
    )
}
