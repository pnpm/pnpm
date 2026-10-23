use std::{net::IpAddr, sync::Arc, time::Instant};

use axum::{
    Router,
    extract::{ConnectInfo, Request, State},
    http::{HeaderName, HeaderValue, Method, header},
    middleware::Next,
    response::Response,
};
use tower_http::compression::{
    CompressionLayer,
    predicate::{DefaultPredicate, NotForContentType, Predicate as _},
};
use tracing::Instrument;

use crate::server::{
    AppState, AuthedCaller, ClientIp, PeerAddr, RequestId, generate_request_id,
    is_valid_request_id, loggable_uri, resolve_client_ip,
};

pub(super) fn with_observability_layers(
    router: Router<AppState>,
    state: AppState,
) -> Router<AppState> {
    let router =
        router.layer(axum::middleware::from_fn_with_state(state, access_and_metrics_middleware));
    with_compression(router)
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

struct PreparedRequest {
    req_id_str: String,
    method: Method,
    uri: String,
    peer: Option<IpAddr>,
    client_ip: Option<IpAddr>,
    user_agent: Option<String>,
    span: tracing::Span,
}

async fn access_and_metrics_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let prep = prepare_request(&mut request, &state);
    let _in_flight = state.inner.metrics.start_in_flight();
    let start = Instant::now();

    let mut response = next.run(request).instrument(prep.span.clone()).await;
    let latency = start.elapsed();
    let status = response.status();

    attach_request_id_header(&mut response, &prep.req_id_str);
    let user = authenticated_username(&response);
    let bytes = response_bytes_count(&response);

    state.inner.metrics.record_request(prep.method.as_str(), status.as_u16(), latency, bytes);
    let entry = AccessLogEntry {
        request_id: &prep.req_id_str,
        method: &prep.method,
        uri: &prep.uri,
        status: status.as_u16(),
        latency_ms: latency.as_millis() as u64,
        bytes,
        user: user.as_deref(),
    };
    emit_access_log(prep.client_ip.or(prep.peer), prep.user_agent.as_deref(), &entry);
    response
}

fn prepare_request(request: &mut Request, state: &AppState) -> PreparedRequest {
    let (req_id_str, req_id) = resolve_request_id(request);
    request.extensions_mut().insert(req_id);

    let (client_ip, peer) = resolve_ips(request, state);
    if let Some(ip) = client_ip {
        request
            .extensions_mut()
            .insert(ClientIp(ip));
    }

    let user_agent = request
        .headers()
        .get(header::USER_AGENT)
        .and_then(|val| val.to_str().ok())
        .map(ToOwned::to_owned);

    let method = request.method().clone();
    let uri = loggable_uri(request.uri());
    let span =
        create_access_span(&req_id_str, &method, &uri, client_ip.or(peer), user_agent.as_deref());

    PreparedRequest { req_id_str, method, uri, peer, client_ip, user_agent, span }
}

fn attach_request_id_header(response: &mut Response, request_id: &str) {
    if let Ok(hdr) = HeaderValue::from_str(request_id) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("x-request-id"), hdr);
    }
}

fn authenticated_username(response: &Response) -> Option<String> {
    response
        .extensions()
        .get::<AuthedCaller>()
        .and_then(|caller| caller.0.username())
        .map(ToOwned::to_owned)
}

struct AccessLogEntry<'a> {
    request_id: &'a str,
    method: &'a Method,
    uri: &'a str,
    status: u16,
    latency_ms: u64,
    bytes: u64,
    user: Option<&'a str>,
}

/// Emits the access record outside the request span. The record carries every
/// field itself, and a formatter that appends the current span's fields to an
/// event (the pretty format, and the JSON format's `span` object) would print
/// each of them a second time.
fn emit_access_log(
    client_ip: Option<IpAddr>,
    user_agent: Option<&str>,
    entry: &AccessLogEntry<'_>,
) {
    let client_ip_display = client_ip.map(|ip| ip.to_string()).unwrap_or_else(|| "-".to_string());
    let user_display = entry.user.unwrap_or("-");
    let user_agent_display = user_agent.unwrap_or("-");

    tracing::info!(
        target: "pnpr::access",
        request_id = %entry.request_id,
        method = %entry.method,
        uri = %entry.uri,
        status = entry.status,
        latency_ms = entry.latency_ms,
        bytes = entry.bytes,
        client_ip = %client_ip_display,
        user = %user_display,
        user_agent = %user_agent_display,
        "finished processing request",
    );
}

fn resolve_request_id(request: &Request) -> (String, RequestId) {
    let request_id = match request.headers().get("x-request-id") {
        Some(value) => match value.to_str() {
            Ok(str_val) if is_valid_request_id(str_val) => str_val.to_string(),
            _ => generate_request_id(),
        },
        None => generate_request_id(),
    };
    let req_id = RequestId(Arc::from(request_id));
    (req_id.as_str().to_string(), req_id)
}

fn resolve_ips(request: &Request, state: &AppState) -> (Option<IpAddr>, Option<IpAddr>) {
    let peer = request
        .extensions()
        .get::<ConnectInfo<PeerAddr>>()
        .map(|ConnectInfo(PeerAddr(addr))| addr.ip());
    let client_ip = resolve_client_ip(
        peer,
        request.headers(),
        &state.inner.config.http.trusted_proxies,
        state.inner.config.http.trusted_proxy_header,
    );
    (client_ip, peer)
}

fn create_access_span(
    request_id: &str,
    method: &Method,
    uri: &str,
    client_ip: Option<IpAddr>,
    user_agent: Option<&str>,
) -> tracing::Span {
    let span = tracing::info_span!(
        target: "pnpr::access",
        "request",
        request_id = %request_id,
        method = %method,
        uri = %uri,
        client_ip = tracing::field::Empty,
        user = tracing::field::Empty,
        user_agent = tracing::field::Empty,
    );
    if let Some(ip) = client_ip {
        span.record("client_ip", tracing::field::display(ip));
    }
    if let Some(ua) = user_agent {
        span.record("user_agent", tracing::field::display(ua));
    }
    span
}

/// Count response bytes derived from the `Content-Length` header.
///
/// Returns 0 when the header is absent or invalid, such as for chunked or streamed responses.
fn response_bytes_count(response: &Response) -> u64 {
    response
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|val| val.to_str().ok())
        .and_then(|len_str| len_str.parse::<u64>().ok())
        .unwrap_or(0)
}
