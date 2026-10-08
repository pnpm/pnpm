//! The admin API for package rules: `/-/pnpr/v0/admin/rules/{ecosystem}/{name}`.
//! Every endpoint requires one of the `auth.admins`.

use super::{
    AppState, AuthedCaller, MAX_LOGIN_BODY_BYTES, RegistryError, Response, StatusCode,
    json_response, private_no_cache,
    registry_directory::management_name,
    require_admin,
    rule_overrides::{RuleChanges, current_rules_version, replace_rules, rules_view},
    team_mutations::respond,
};
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, HeaderValue, header},
    response::IntoResponse,
    routing::get,
};
use pnpr_config::{HostedConfig, Management};
use pnpr_registry::Ecosystem;
use serde::Deserialize;

pub(super) fn rule_admin_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/-/pnpr/v0/admin/rules/{ecosystem}/{name}",
            get(get_rules)
                .put(put_rules)
                .delete(delete_rules)
                .route_layer(DefaultBodyLimit::max(MAX_LOGIN_BODY_BYTES)),
        )
}

/// A registry as the registry directory names it: its ecosystem and its
/// name within that ecosystem.
#[derive(Deserialize)]
struct RegistryPath {
    ecosystem: String,
    name: String,
}

/// `GET /-/pnpr/v0/admin/rules/{ecosystem}/{name}` — the hosted registry's
/// current rules, whether the admin API may change them, and the version of
/// the stored changes as `version` and `ETag`.
async fn get_rules(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(path): Path<RegistryPath>,
) -> Response {
    let result = async {
        require_admin(&state, &identity, "read", "the package rules".to_string())?;
        let (registry, hosted) = hosted_registry(&state, &path)?;
        let version = current_rules_version(&state, registry).await?;
        let mut view = rules_view(&hosted.rules.snapshot());
        view["version"] = serde_json::json!(version);
        view["rulesManagedBy"] = serde_json::json!(management_name(hosted.rules_managed_by));
        Ok(versioned(json_response(StatusCode::OK, &view), &version))
    };
    private_no_cache(respond(result.await))
}

/// `PUT /-/pnpr/v0/admin/rules/{ecosystem}/{name}` — replace the stored
/// changes with the body's, and answer with the rules that result. With
/// `If-Match`, only while the stored changes still have that version.
async fn put_rules(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(path): Path<RegistryPath>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let result = async {
        let (registry, hosted) = managed_registry(&state, &identity, &path)?;
        let changes: RuleChanges = serde_json::from_slice(&body)
            .map_err(|err| RegistryError::BadRequest {
                reason: format!("invalid request body: {err}"),
            })?;
        let version =
            replace_rules(&state, registry, changes, if_match(&headers).as_deref()).await?;
        let mut view = rules_view(&hosted.rules.snapshot());
        view["version"] = serde_json::json!(version);
        let response = json_response(StatusCode::OK, &view);
        Ok(versioned(response, &version))
    };
    respond(result.await)
}

/// `DELETE /-/pnpr/v0/admin/rules/{ecosystem}/{name}` — drop the stored
/// changes, so the configuration's rules apply again. With `If-Match`, only
/// while the stored changes still have that version.
async fn delete_rules(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(path): Path<RegistryPath>,
    headers: HeaderMap,
) -> Response {
    let result = async {
        let (registry, _) = managed_registry(&state, &identity, &path)?;
        let expected = if_match(&headers);
        let version =
            replace_rules(&state, registry, RuleChanges::default(), expected.as_deref()).await?;
        Ok(versioned(StatusCode::NO_CONTENT.into_response(), &version))
    };
    respond(result.await)
}

/// The version an `If-Match` header names, or `None` for no condition (no
/// header, or `*`).
fn if_match(headers: &HeaderMap) -> Option<String> {
    let value = headers
        .get(header::IF_MATCH)?
        .to_str()
        .ok()?
        .trim();
    let value = value
        .strip_prefix("W/")
        .unwrap_or(value)
        .trim_matches('"');
    (value != "*").then(|| value.to_string())
}

fn versioned(mut response: Response, version: &str) -> Response {
    if let Ok(etag) = HeaderValue::from_str(&format!(r#""{version}""#)) {
        response.headers_mut().insert(header::ETAG, etag);
    }
    response
}

/// The hosted registry whose rules an admin may change, with its key.
fn managed_registry<'a>(
    state: &'a AppState,
    identity: &pnpr_policy::Identity,
    path: &RegistryPath,
) -> Result<(&'a str, &'a HostedConfig), RegistryError> {
    require_admin(state, identity, "change", "the package rules".to_string())?;
    let (registry, hosted) = hosted_registry(state, path)?;
    if hosted.rules_managed_by == Management::Config {
        return Err(RegistryError::RulesConfigManaged);
    }
    Ok((registry, hosted))
}

/// The hosted registry `path` names, with its key.
fn hosted_registry<'a>(
    state: &'a AppState,
    path: &RegistryPath,
) -> Result<(&'a str, &'a HostedConfig), RegistryError> {
    let routing = &state.inner.config.routing;
    let ecosystem = Ecosystem::all()
        .find(|ecosystem| ecosystem.as_str() == path.ecosystem)
        .ok_or(RegistryError::NotFound)?;
    let registry =
        routing.registries.addressed(&path.name, ecosystem).ok_or(RegistryError::NotFound)?;
    routing.hosted
        .get_key_value(registry)
        .map(|(key, hosted)| (key.as_str(), hosted))
        .ok_or(RegistryError::NotFound)
}
