mod encoding;
mod legacy;
mod legacy_merge;
mod legacy_nodes;
mod legacy_report;
mod legacy_sanitize;
mod legacy_specs;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::post,
};
use indexmap::IndexMap;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_policy::Identity;
use pnpr_upstream::{FetchOutcome, Upstream};
use reqwest::{StatusCode, header};
use serde_json::{Map, Value};

use super::{
    Action, AppState, AuthedCaller, HostedGate, RegistryError, RegistrySource, TargetRegistry,
    addressed_registry, authorize, authorized_upstream,
    ecosystem::{hosted_sources, upstream_sources},
    hosted_gate, not_found, resolve_registry_source,
};

const MAX_AUDIT_BODY_BYTES: usize = 16 * 1024 * 1024;
const CACHE_CONTROL_NO_STORE: &str = "private, no-cache, no-store, must-revalidate";

type BulkPackageQuery = HashMap<String, Vec<String>>;

pub(super) enum PackageDestination {
    Hosted,
    Upstream(String),
    Skip,
}

struct BulkPartition {
    osv_packages: BulkPackageQuery,
    by_upstream: IndexMap<String, BulkPackageQuery>,
}

struct PreparedBulkUpstream {
    client: Upstream,
    subset: BulkPackageQuery,
    payload: Vec<u8>,
}

struct BulkUpstreamOutcome {
    subset: BulkPackageQuery,
    outcome: FetchOutcome<Vec<u8>>,
}

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
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(target) = addressed_registry(&state, registry.as_deref(), Ecosystem::Npm) else {
        return not_found();
    };
    let packages = match parse_bulk_request(&headers, &body) {
        Ok(parsed) => parsed,
        Err(response) => return response,
    };
    let is_empty_query = packages.is_empty();
    let partition = match partition_bulk_packages(&state, &identity, &target, packages) {
        Ok(partition) => partition,
        Err(err) => return err.into_response(),
    };

    if let Some(osv_index) = state.inner.osv_index.as_ref() {
        let advisories = osv_index.bulk_advisories(&partition.osv_packages);
        return json_response(&advisories);
    }
    if partition.by_upstream.is_empty() {
        return empty_bulk_response(&state, &identity, &target, is_empty_query);
    }
    forward_partitioned_bulk(&state, &identity, partition.by_upstream).await
}

fn parse_bulk_request(headers: &HeaderMap, body: &Bytes) -> Result<BulkPackageQuery, Response> {
    let decoded = encoding::decode_request_body(headers, body)?;
    if decoded.is_empty() {
        return Ok(HashMap::new());
    }
    serde_json::from_slice(&decoded).map_err(|_| bad_request_json())
}

fn empty_bulk_response(
    state: &AppState,
    identity: &Identity,
    target: &str,
    is_empty_query: bool,
) -> Response {
    let upstreams = upstream_sources(state, target, Ecosystem::Npm);
    if is_empty_query
        && upstreams.len() == 1
        && hosted_sources(state, target, Ecosystem::Npm).is_empty()
        && let Err(err) = authorized_upstream(state, identity, &upstreams[0])
    {
        return err.into_response();
    }
    json_response(&HashMap::<String, Vec<pnpr_osv::BulkAdvisory>>::new())
}

pub(super) fn classify_package(
    state: &AppState,
    identity: &Identity,
    target: &str,
    raw_name: &str,
) -> Result<PackageDestination, RegistryError> {
    let Ok(canonical) = CanonicalPackageName::parse(raw_name, Ecosystem::Npm) else {
        return Ok(PackageDestination::Skip);
    };
    let resolved = resolve_registry_source(state, target, canonical.as_str());
    match &resolved {
        RegistrySource::Hosted(source) => {
            match hosted_gate(state, identity, source, canonical.as_str()) {
                HostedGate::Allowed(_) => Ok(PackageDestination::Hosted),
                HostedGate::MaskNotFound => Ok(PackageDestination::Skip),
                HostedGate::Denied(err) => Err(err),
            }
        }
        RegistrySource::Upstream(source) => {
            authorize(state, identity, &resolved, canonical.as_str(), Action::Read)?;
            Ok(PackageDestination::Upstream(source.clone()))
        }
        RegistrySource::Unclaimed | RegistrySource::NotFound => Ok(PackageDestination::Skip),
    }
}

fn partition_bulk_packages(
    state: &AppState,
    identity: &Identity,
    target: &str,
    packages: HashMap<String, Vec<String>>,
) -> Result<BulkPartition, RegistryError> {
    let ordered_upstreams = upstream_sources(state, target, Ecosystem::Npm);
    let mut osv_packages = HashMap::new();
    let mut raw_by_upstream: HashMap<String, HashMap<String, Vec<String>>> = HashMap::new();

    for (package_name, versions) in packages {
        match classify_package(state, identity, target, &package_name)? {
            PackageDestination::Hosted => {
                osv_packages.insert(package_name, versions);
            }
            PackageDestination::Upstream(upstream_name) => {
                osv_packages.insert(package_name.clone(), versions.clone());
                raw_by_upstream
                    .entry(upstream_name)
                    .or_default()
                    .insert(package_name, versions);
            }
            PackageDestination::Skip => {}
        }
    }

    let mut by_upstream = IndexMap::new();
    for upstream_name in ordered_upstreams {
        if let Some(subset) = raw_by_upstream.remove(&upstream_name) {
            by_upstream.insert(upstream_name, subset);
        }
    }
    Ok(BulkPartition { osv_packages, by_upstream })
}

async fn forward_partitioned_bulk(
    state: &AppState,
    identity: &Identity,
    by_upstream: IndexMap<String, BulkPackageQuery>,
) -> Response {
    let mut prepared = Vec::with_capacity(by_upstream.len());
    for (upstream_name, subset) in by_upstream {
        let client = match authorized_upstream(state, identity, &upstream_name) {
            Ok(client) => client,
            Err(err) => return err.into_response(),
        };
        let payload = match serde_json::to_vec(&subset) {
            Ok(bytes) => bytes,
            Err(err) => return RegistryError::Json(err).into_response(),
        };
        prepared.push(PreparedBulkUpstream { client, subset, payload });
    }

    let futures = prepared
        .into_iter()
        .map(|item| async move {
            let outcome =
                item.client.post_audit("/-/npm/v1/security/advisories/bulk", &item.payload).await?;
            Ok::<_, RegistryError>(BulkUpstreamOutcome { subset: item.subset, outcome })
        });
    let outcomes = match futures_util::future::try_join_all(futures).await {
        Ok(outcomes) => outcomes,
        Err(err) => return err.into_response(),
    };

    merge_bulk_upstream_outcomes(outcomes)
}

fn merge_bulk_upstream_outcomes(outcomes: Vec<BulkUpstreamOutcome>) -> Response {
    let mut merged = Map::new();
    for entry in outcomes {
        let FetchOutcome::Ok(bytes) = entry.outcome else {
            return not_found();
        };
        let parsed: Map<String, Value> = match serde_json::from_slice(&bytes) {
            Ok(map) => map,
            Err(err) => return RegistryError::Json(err).into_response(),
        };
        insert_provenance_filtered_bulk(parsed, &entry.subset, &mut merged);
    }
    json_response(&merged)
}

fn insert_provenance_filtered_bulk(
    parsed: Map<String, Value>,
    subset: &BulkPackageQuery,
    merged: &mut Map<String, Value>,
) {
    for (package_name, advisories) in parsed {
        if subset.contains_key(&package_name) && advisories.is_array() {
            merged.insert(package_name, advisories);
        }
    }
}

async fn post_security_audits(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    legacy::serve_legacy_audit(
        &state,
        &identity,
        &headers,
        registry.as_deref(),
        "/-/npm/v1/security/audits",
        &body,
    )
    .await
}

async fn post_security_audits_quick(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    legacy::serve_legacy_audit(
        &state,
        &identity,
        &headers,
        registry.as_deref(),
        "/-/npm/v1/security/audits/quick",
        &body,
    )
    .await
}

pub(super) fn json_response<Payload: serde::Serialize>(value: &Payload) -> Response {
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
            (header::CACHE_CONTROL, CACHE_CONTROL_NO_STORE),
        ],
        bytes,
    )
        .into_response()
}

pub(super) fn bad_request_json() -> Response {
    audit_error_response(StatusCode::BAD_REQUEST, "invalid json payload")
}

pub(super) fn audit_error_response(status: StatusCode, message: &str) -> Response {
    let mut response = pnpr_error::json_error(status, message, None);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, header::HeaderValue::from_static(CACHE_CONTROL_NO_STORE));
    response
}
