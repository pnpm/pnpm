use std::collections::HashMap;

use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    response::{IntoResponse, Response},
    routing::post,
};
use reqwest::{StatusCode, header};

use pnpr_package_name::Ecosystem;
use pnpr_policy::Identity;
use pnpr_upstream::FetchOutcome;

use super::{
    AppState, AuthedCaller, RegistryError, TargetRegistry, addressed_registry, authorized_upstream,
    ecosystem::upstream_sources, not_found,
};

const MAX_AUDIT_BODY_BYTES: usize = 16 * 1024 * 1024;

pub(super) fn routes(router: Router<AppState>, base: &str) -> Router<AppState> {
    let path = |tail: &str| format!("{base}{tail}");
    let limit = DefaultBodyLimit::max(MAX_AUDIT_BODY_BYTES);
    router
        .route(
            &path("/-/npm/v1/security/advisories/bulk"),
            post(post_bulk_advisories).route_layer(limit),
        )
        .route(&path("/-/npm/v1/security/audits"), post(post_security_audits).route_layer(limit))
        .route(
            &path("/-/npm/v1/security/audits/quick"),
            post(post_security_audits_quick).route_layer(limit),
        )
}

async fn post_bulk_advisories(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    body: Bytes,
) -> Response {
    let Some(target) = addressed_registry(&state, registry.as_deref(), Ecosystem::Npm) else {
        return not_found();
    };

    let packages: HashMap<String, Vec<String>> = if body.is_empty() {
        HashMap::new()
    } else {
        match serde_json::from_slice(&body) {
            Ok(parsed) => parsed,
            Err(_) => return bad_request_json(),
        }
    };

    if let Some(osv_index) = state.inner.osv_index.as_ref() {
        let advisories = osv_index.bulk_advisories(&packages);
        return json_response(&advisories);
    }

    forward_bulk_to_upstream(&state, &identity, &target, &body).await
}

async fn forward_bulk_to_upstream(
    state: &AppState,
    identity: &Identity,
    target: &str,
    body: &[u8],
) -> Response {
    let upstreams = upstream_sources(state, target, Ecosystem::Npm);
    let Some(upstream_name) = upstreams.first() else {
        return json_response(&HashMap::<String, Vec<pnpr_osv::BulkAdvisory>>::new());
    };

    let upstream = match authorized_upstream(state, identity, upstream_name) {
        Ok(client) => client,
        Err(err) => return err.into_response(),
    };

    let payload = if body.is_empty() { b"{}" } else { body };
    match upstream.post_audit("/-/npm/v1/security/advisories/bulk", payload).await {
        Ok(FetchOutcome::Ok(bytes)) => audit_bytes_response(bytes),
        Ok(FetchOutcome::NotFound) => not_found(),
        Err(err) => err.into_response(),
    }
}

async fn post_security_audits(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    body: Bytes,
) -> Response {
    forward_legacy_audit(&state, &identity, registry.as_deref(), "/-/npm/v1/security/audits", &body)
        .await
}

async fn post_security_audits_quick(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    body: Bytes,
) -> Response {
    forward_legacy_audit(
        &state,
        &identity,
        registry.as_deref(),
        "/-/npm/v1/security/audits/quick",
        &body,
    )
    .await
}

async fn forward_legacy_audit(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    endpoint: &str,
    body: &[u8],
) -> Response {
    let Some(target) = addressed_registry(state, registry, Ecosystem::Npm) else {
        return not_found();
    };

    let upstreams = upstream_sources(state, &target, Ecosystem::Npm);
    let Some(upstream_name) = upstreams.first() else {
        return empty_legacy_audit_response();
    };

    let upstream = match authorized_upstream(state, identity, upstream_name) {
        Ok(client) => client,
        Err(err) => return err.into_response(),
    };

    let payload = if body.is_empty() { b"{}" } else { body };
    match upstream.post_audit(endpoint, payload).await {
        Ok(FetchOutcome::Ok(bytes)) => audit_bytes_response(bytes),
        Ok(FetchOutcome::NotFound) => empty_legacy_audit_response(),
        Err(err) => err.into_response(),
    }
}

fn empty_legacy_audit_response() -> Response {
    let body = serde_json::json!({
        "actions": [],
        "advisories": {},
        "muted": [],
        "metadata": {
            "vulnerabilities": {
                "info": 0,
                "low": 0,
                "moderate": 0,
                "high": 0,
                "critical": 0,
                "total": 0,
            },
            "dependencies": 0,
            "devDependencies": 0,
            "optionalDependencies": 0,
            "totalDependencies": 0,
        },
    });
    json_response(&body)
}

fn json_response<Payload: serde::Serialize>(value: &Payload) -> Response {
    let body = match serde_json::to_vec(value) {
        Ok(bytes) => bytes,
        Err(err) => return RegistryError::Json(err).into_response(),
    };
    audit_bytes_response(body)
}

fn audit_bytes_response(bytes: Vec<u8>) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "private, no-cache, no-store, must-revalidate"),
        ],
        bytes,
    )
        .into_response()
}

fn bad_request_json() -> Response {
    (
        StatusCode::BAD_REQUEST,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "private, no-cache, no-store, must-revalidate"),
        ],
        r#"{"error":"invalid json payload"}"#,
    )
        .into_response()
}
