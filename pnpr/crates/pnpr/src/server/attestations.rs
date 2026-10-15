use axum::{
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;

use pnpr_error::RegistryError;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_policy::Identity;
use pnpr_storage::publish::extract_predicate_type;
use pnpr_upstream::FetchOutcome;

use super::{
    Action, AppState, AuthedCaller, RegistrySource, TargetRegistry, addressed_registry, authorize,
    authorized_upstream, ecosystem::upstream_sources, ensure_osv_allowed, hosted_read_namespace,
    not_found, private_no_cache, resolve_registry_source,
};

#[derive(Deserialize)]
pub(super) struct AttestationsPath {
    pub(super) spec: String,
}

/// `GET /-/npm/v1/attestations/{*spec}` or `GET /~{registry}/-/npm/v1/attestations/{*spec}`.
pub(super) async fn get_attestations(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<AttestationsPath>,
) -> Response {
    let spec = path.spec.trim_start_matches('/');
    private_no_cache(serve_attestations(&state, &identity, registry.as_deref(), spec).await)
}

/// `GET /-/npm/v1/keys` or `GET /~{registry}/-/npm/v1/keys`.
pub(super) async fn get_keys(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
) -> Response {
    private_no_cache(serve_keys(&state, &identity, registry.as_deref()).await)
}

async fn serve_attestations(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    spec: &str,
) -> Response {
    let Some((raw_pkg, version)) = parse_attestation_spec(spec) else {
        return not_found();
    };
    let Ok(name) = CanonicalPackageName::parse(&raw_pkg, Ecosystem::Npm) else {
        return not_found();
    };
    let Some(target) = addressed_registry(state, registry, Ecosystem::Npm) else {
        return not_found();
    };
    let resolved_source = resolve_registry_source(state, &target, name.as_str());
    match &resolved_source {
        RegistrySource::Hosted(source) => {
            serve_hosted_attestations(state, identity, source, &name, &version).await
        }
        RegistrySource::Upstream(source) => {
            if let Err(err) =
                authorize(state, identity, &resolved_source, name.as_str(), Action::Access)
            {
                return err.into_response();
            }
            let upstream_spec = format!("{}@{version}", name.as_str());
            serve_upstream_attestations(state, identity, source, &upstream_spec).await
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => not_found(),
    }
}

fn parse_attestation_spec(spec: &str) -> Option<(String, String)> {
    let idx = spec.rfind('@')?;
    if idx == 0 {
        return None;
    }
    let raw_pkg = &spec[..idx];
    let version = &spec[idx + 1..];
    if version.is_empty() || version.contains(['/', '\\', '?', '#']) || version.contains("..") {
        return None;
    }
    let decoded_pkg = pnpr_search::percent_decode(raw_pkg);
    Some((decoded_pkg, version.to_string()))
}

async fn serve_hosted_attestations(
    state: &AppState,
    identity: &Identity,
    source: &str,
    name: &CanonicalPackageName,
    version: &str,
) -> Response {
    let org = match hosted_read_namespace(state, identity, source, name.as_str()) {
        Ok(org) => org,
        Err(err) => return err.into_response(),
    };
    if let Err(err) = ensure_osv_allowed(state, name, version) {
        return err.into_response();
    }
    let filename = name.sigstore_name_for_version(version);
    match state.inner.storage.for_hosted(&org).open_hosted_blob(name, &filename).await {
        Ok(Some((body, _len))) => {
            let bytes = match axum::body::to_bytes(body, 16 * 1024 * 1024).await {
                Ok(bytes) => bytes,
                Err(err) => return RegistryError::Io(std::io::Error::other(err)).into_response(),
            };
            let bundle: serde_json::Value = match serde_json::from_slice(&bytes) {
                Ok(val) => val,
                Err(err) => return RegistryError::Json(err).into_response(),
            };
            let predicate_type = extract_predicate_type(&String::from_utf8_lossy(&bytes));
            let response_body = json!({
                "attestations": [
                    {
                        "predicateType": predicate_type,
                        "bundle": bundle,
                    }
                ]
            });
            let payload =
                serde_json::to_vec(&response_body).expect("attestations response serializes");
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(payload))
                .expect("response builds")
        }
        Ok(None) => not_found(),
        Err(err) => err.into_response(),
    }
}

async fn serve_upstream_attestations(
    state: &AppState,
    identity: &Identity,
    source: &str,
    spec: &str,
) -> Response {
    let upstream = match authorized_upstream(state, identity, source) {
        Ok(u) => u,
        Err(err) => return err.into_response(),
    };
    match upstream.get_attestations(spec).await {
        Ok(FetchOutcome::Ok(bytes)) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(bytes))
            .expect("upstream attestations response builds"),
        Ok(FetchOutcome::NotFound) => not_found(),
        Err(err) => err.into_response(),
    }
}

async fn serve_keys(state: &AppState, identity: &Identity, registry: Option<&str>) -> Response {
    let Some(target) = addressed_registry(state, registry, Ecosystem::Npm) else {
        return not_found();
    };
    let upstreams = upstream_sources(state, &target, Ecosystem::Npm);
    if let Some(upstream_name) = upstreams.first() {
        let upstream = match authorized_upstream(state, identity, upstream_name) {
            Ok(u) => u,
            Err(err) => return err.into_response(),
        };
        match upstream.get_keys().await {
            Ok(FetchOutcome::Ok(bytes)) => {
                return Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(bytes))
                    .expect("upstream keys response builds");
            }
            Ok(FetchOutcome::NotFound) => {}
            Err(err) => return err.into_response(),
        }
    }
    empty_keys_response()
}

fn empty_keys_response() -> Response {
    let payload = serde_json::to_vec(&json!({ "keys": [] })).expect("empty keys serializes");
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(payload))
        .expect("empty keys response builds")
}
