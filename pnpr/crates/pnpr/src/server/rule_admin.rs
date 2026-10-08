//! The admin API for package rules: `/-/pnpr/v0/admin/rules/{ecosystem}/{name}`.
//! Every endpoint requires one of the `auth.admins`.

use super::{
    AppState, AuthedCaller, MAX_LOGIN_BODY_BYTES, RegistryError, Response, StatusCode,
    json_response, private_no_cache,
    registry_directory::management_name,
    require_admin,
    rule_overrides::{RuleChanges, replace_rules, reset_rules, rules_view},
    team_mutations::respond,
};
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
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

/// `GET /-/pnpr/v0/admin/rules/{ecosystem}/{name}` — the hosted registry's current
/// rules, and whether the admin API may change them.
async fn get_rules(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(path): Path<RegistryPath>,
) -> Response {
    let result = (|| {
        require_admin(&state, &identity, "read", "the package rules".to_string())?;
        let (_, hosted) = hosted_registry(&state, &path)?;
        let mut view = rules_view(&hosted.rules.snapshot());
        view["rulesManagedBy"] = serde_json::json!(management_name(hosted.rules_managed_by));
        Ok(json_response(StatusCode::OK, &view))
    })();
    private_no_cache(respond(result))
}

/// `PUT /-/pnpr/v0/admin/rules/{ecosystem}/{name}` — replace the stored changes with
/// the body's, and answer with the rules that result.
async fn put_rules(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(path): Path<RegistryPath>,
    body: Bytes,
) -> Response {
    let result = async {
        let (registry, hosted) = managed_registry(&state, &identity, &path)?;
        let changes: RuleChanges = serde_json::from_slice(&body)
            .map_err(|err| RegistryError::BadRequest {
                reason: format!("invalid request body: {err}"),
            })?;
        replace_rules(&state, registry, changes).await?;
        Ok(json_response(StatusCode::OK, &rules_view(&hosted.rules.snapshot())))
    };
    respond(result.await)
}

/// `DELETE /-/pnpr/v0/admin/rules/{ecosystem}/{name}` — drop the stored changes, so
/// the configuration's rules apply again.
async fn delete_rules(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(path): Path<RegistryPath>,
) -> Response {
    let result = async {
        let (registry, _) = managed_registry(&state, &identity, &path)?;
        reset_rules(&state, registry).await?;
        Ok(StatusCode::NO_CONTENT)
    };
    respond(result.await)
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
