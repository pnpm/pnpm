use std::collections::{HashMap, HashSet};

use axum::{
    body::Bytes,
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use pnpr_error::RegistryError;
use pnpr_package_name::Ecosystem;
use pnpr_policy::Identity;
use pnpr_upstream::{FetchOutcome, Upstream};
use serde_json::{Map, Value};

use super::{
    AppState, PackageDestination, addressed_registry, authorized_upstream, bad_request_json,
    encoding::decode_request_body,
    json_response,
    legacy_merge::{merge_upstream_legacy_responses, sanitize_upstream_legacy_response},
    legacy_nodes::{LegacyClassification, classify_legacy_packages, collect_installed_nodes},
    legacy_report::{InstalledNode, LegacyAuditReport, build_osv_legacy_report},
    legacy_sanitize::build_upstream_legacy_payload,
    not_found, upstream_sources,
};

struct PreparedLegacyUpstream {
    client: Upstream,
    allowed: HashSet<String>,
    payload: Vec<u8>,
}

pub(super) async fn serve_legacy_audit(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    registry: Option<&str>,
    endpoint: &str,
    body: &Bytes,
) -> Response {
    let Some(target) = addressed_registry(state, registry, Ecosystem::Npm) else {
        return not_found();
    };
    let decoded = match decode_request_body(headers, body) {
        Ok(bytes) => bytes,
        Err(response) => return response,
    };
    let payload: Map<String, Value> = if decoded.is_empty() {
        Map::new()
    } else {
        match serde_json::from_slice(&decoded) {
            Ok(parsed) => parsed,
            Err(_) => return bad_request_json(),
        }
    };

    let Ok(raw_nodes) = collect_installed_nodes(&payload) else {
        return bad_request_json();
    };
    let classification =
        match classify_legacy_packages(state, identity, &target, &payload, &raw_nodes) {
            Ok(classification) => classification,
            Err(err) => return err.into_response(),
        };

    if let Some(osv_index) = state.inner.osv_index.as_ref() {
        let allowed_nodes = retain_osv_nodes(raw_nodes, &classification.destinations);
        return json_response(&build_osv_legacy_report(osv_index, &allowed_nodes));
    }

    forward_partitioned_legacy(state, identity, &target, endpoint, &payload, &classification).await
}

fn retain_osv_nodes(
    raw_nodes: Vec<InstalledNode>,
    destinations: &HashMap<String, PackageDestination>,
) -> Vec<InstalledNode> {
    raw_nodes
        .into_iter()
        .filter(|node| match destinations.get(&node.package) {
            Some(PackageDestination::Hosted | PackageDestination::Upstream(_)) => true,
            Some(PackageDestination::Skip) | None => false,
        })
        .collect()
}

async fn forward_partitioned_legacy(
    state: &AppState,
    identity: &Identity,
    target: &str,
    endpoint: &str,
    payload: &Map<String, Value>,
    classification: &LegacyClassification,
) -> Response {
    let active_upstreams =
        select_legacy_upstreams(state, target, payload, &classification.referenced_upstreams);
    if active_upstreams.is_empty() {
        return json_response(&LegacyAuditReport::default());
    }

    let mut prepared = Vec::with_capacity(active_upstreams.len());
    for upstream_name in active_upstreams {
        let client = match authorized_upstream(state, identity, &upstream_name) {
            Ok(client) => client,
            Err(err) => return err.into_response(),
        };
        let sanitized =
            build_upstream_legacy_payload(payload, &upstream_name, &classification.destinations);
        let serialized = match serde_json::to_vec(&sanitized) {
            Ok(bytes) => bytes,
            Err(err) => return RegistryError::Json(err).into_response(),
        };
        let allowed = allowed_packages_for_upstream(&upstream_name, &classification.destinations);
        prepared.push(PreparedLegacyUpstream { client, allowed, payload: serialized });
    }

    execute_legacy_upstream_requests(endpoint, prepared).await
}

async fn execute_legacy_upstream_requests(
    endpoint: &str,
    prepared: Vec<PreparedLegacyUpstream>,
) -> Response {
    let is_single = prepared.len() == 1;
    let futures = prepared
        .into_iter()
        .map(|item| async move {
            let outcome = item.client.post_audit(endpoint, &item.payload).await?;
            Ok::<_, RegistryError>((item.allowed, outcome))
        });
    let outcomes = match futures_util::future::try_join_all(futures).await {
        Ok(outcomes) => outcomes,
        Err(err) => return err.into_response(),
    };

    let mut parsed_docs = Vec::new();
    for (allowed, outcome) in outcomes {
        if let FetchOutcome::Ok(bytes) = outcome {
            let doc: Map<String, Value> = match serde_json::from_slice(&bytes) {
                Ok(doc) => doc,
                Err(err) => return RegistryError::Json(err).into_response(),
            };
            parsed_docs.push((allowed, doc));
        }
    }

    if parsed_docs.is_empty() {
        return json_response(&LegacyAuditReport::default());
    }
    if is_single && let Some((allowed, mut doc)) = parsed_docs.pop() {
        sanitize_upstream_legacy_response(&mut doc, &allowed);
        return json_response(&doc);
    }
    let merged = merge_upstream_legacy_responses(parsed_docs);
    json_response(&merged)
}

fn select_legacy_upstreams(
    state: &AppState,
    target: &str,
    payload: &Map<String, Value>,
    referenced_upstreams: &HashSet<String>,
) -> Vec<String> {
    let configured = upstream_sources(state, target, Ecosystem::Npm);
    if referenced_upstreams.is_empty()
        && !payload_has_any_dependency_entries(payload)
        && configured.len() == 1
    {
        return configured;
    }

    configured
        .into_iter()
        .filter(|upstream_name| referenced_upstreams.contains(upstream_name.as_str()))
        .collect()
}

fn payload_has_any_dependency_entries(payload: &Map<String, Value>) -> bool {
    let has_requires = payload
        .get("requires")
        .and_then(Value::as_object)
        .is_some_and(|map| !map.is_empty());
    let has_deps = payload
        .get("dependencies")
        .and_then(Value::as_object)
        .is_some_and(|map| !map.is_empty());
    let has_packages = payload
        .get("packages")
        .and_then(Value::as_object)
        .is_some_and(|map| map.keys().any(|key| !key.is_empty()));
    let has_arrays = ["install", "remove"]
        .iter()
        .any(|key| {
            payload
                .get(*key)
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty())
        });
    has_requires || has_deps || has_packages || has_arrays
}

fn allowed_packages_for_upstream(
    upstream_name: &str,
    destinations: &HashMap<String, PackageDestination>,
) -> HashSet<String> {
    destinations
        .iter()
        .filter_map(|(package, destination)| match destination {
            PackageDestination::Upstream(owner) if owner == upstream_name => Some(package.clone()),
            PackageDestination::Hosted
            | PackageDestination::Upstream(_)
            | PackageDestination::Skip => None,
        })
        .collect()
}
