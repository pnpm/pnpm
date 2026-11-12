//! The admin API for package rules: `/-/pnpr/v0/admin/rules/{ecosystem}/{name}`.
//! Every endpoint requires one of the `auth.admins` or `auth.registryAdmins`.
//! Changing the rules needs `registriesManagedBy: api`; a change edits the
//! stored registry graph.

use super::{
    AppState, AuthedCaller, MAX_LOGIN_BODY_BYTES, RegistryError, Response, StatusCode,
    json_response, parse_json_body, private_no_cache,
    registry_graph::{
        Credentials, GraphEdit, edit_graph, graph_version, if_match, stored_document,
        stored_routing, versioned,
    },
    request_access::require_access_admin,
    team_mutations::respond,
};
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::HeaderMap,
    routing::get,
};
use pnpr_config::{HostedConfig, Management, RegistryGraphDocument, RoutingConfig, RuleLists};
use pnpr_policy::{AccessList, RuleTable};
use pnpr_registry::{Ecosystem, PackagePattern};
use serde::Deserialize;
use std::{collections::BTreeMap, sync::Arc};

pub(super) fn rule_admin_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/-/pnpr/v0/admin/rules/{ecosystem}/{name}",
            get(get_rules)
                .put(put_rules)
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

/// What an admin changes in a registry's rules. Each list replaces the list
/// of the same place; an omitted list keeps it.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleChanges {
    /// The registry-level `access:`.
    #[serde(default)]
    access: Option<Vec<String>>,
    /// Keyed by a pattern the registry's `packages:` map declares.
    #[serde(default)]
    packages: BTreeMap<String, PackageChanges>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageChanges {
    #[serde(default)]
    access: Option<Vec<String>>,
    #[serde(default)]
    publish: Option<Vec<String>>,
    #[serde(default)]
    unpublish: Option<Vec<String>>,
}

/// `GET /-/pnpr/v0/admin/rules/{ecosystem}/{name}`: the hosted registry's
/// current rules, who manages them, and the registry graph version as
/// `version` and `ETag`.
async fn get_rules(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(path): Path<RegistryPath>,
) -> Response {
    let result = async {
        require_access_admin(&state, &identity, "read", "the package rules".to_string())?;
        let (rules, version) = current_rules(&state, &path).await?;
        Ok(versioned(
            json_response(StatusCode::OK, &rules_body(&state, &rules, &version)),
            &version,
        ))
    };
    private_no_cache(respond(result.await))
}

/// `PUT /-/pnpr/v0/admin/rules/{ecosystem}/{name}`: change the lists the body
/// names, and answer with the rules that result. With `If-Match`, only while
/// the registry graph still has that version.
async fn put_rules(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(path): Path<RegistryPath>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let result = async {
        require_access_admin(&state, &identity, "change", "the package rules".to_string())?;
        if state.inner.config.registries_managed_by == Management::Config {
            return Err(RegistryError::RulesConfigManaged);
        }
        let registry = stored_registry_key(&state, &path).await?;
        let expected = if_match(&headers)?;
        let changes: RuleChanges = parse_json_body(&body)?;
        let request =
            GraphEdit::latest(crate::audit::Actor::of(&identity)).expecting(expected.as_deref());
        let outcome = edit_graph(&state, request, |document| {
            let ecosystem =
                document.hosted_storage(&registry).ok_or(RegistryError::NotFound)?.ecosystem;
            apply_rule_changes(document, &registry, ecosystem, &changes)
        })
        .await?;
        let actor = crate::audit::Actor::of(&identity);
        crate::audit!(
            actor,
            "rules.replace",
            ecosystem = %path.ecosystem,
            registry = %path.name,
            version = %outcome.version,
            "package rules replaced",
        );
        let hosted = outcome.routing.hosted.get(&registry).ok_or(RegistryError::NotFound)?;
        let body = rules_body(&state, &hosted.rules.snapshot(), &outcome.version);
        Ok(versioned(json_response(StatusCode::OK, &body), &outcome.version))
    };
    respond(result.await)
}

/// The rules of the registry `path` names, with their version. With
/// `registriesManagedBy: api` they come from the stored graph, so every
/// replica answers with the version an `If-Match` is checked against.
async fn current_rules(
    state: &AppState,
    path: &RegistryPath,
) -> Result<(Arc<RuleTable>, String), RegistryError> {
    if state.inner.config.registries_managed_by == Management::Config {
        let routing = state.routing();
        let (_, hosted) = hosted_registry(&routing, path)?;
        return Ok((hosted.rules.snapshot(), "config".to_string()));
    }
    let (routing, version) = stored_rules_routing(state).await?;
    let (_, hosted) = hosted_registry(&routing, path)?;
    Ok((hosted.rules.snapshot(), version))
}

/// The key of the hosted registry `path` names in the stored graph, which a
/// change applies to even before this replica serves that graph.
async fn stored_registry_key(
    state: &AppState,
    path: &RegistryPath,
) -> Result<String, RegistryError> {
    let (routing, _) = stored_rules_routing(state).await?;
    Ok(hosted_registry(&routing, path)?.0.to_string())
}

/// The routing of the stored graph, with its version, for the rules it holds.
async fn stored_rules_routing(state: &AppState) -> Result<(RoutingConfig, String), RegistryError> {
    let stored = state.inner.storage.read_registry_graph().await?;
    let version = graph_version(stored.as_ref().map(|stored| stored.generation));
    let document = stored_document(stored.as_ref())?;
    let routing = stored_routing(&state.inner.config, &document, Credentials::Skip)?;
    Ok((routing, version))
}

fn rules_body(state: &AppState, rules: &RuleTable, version: &str) -> serde_json::Value {
    let mut view = rules_view(rules);
    view["version"] = serde_json::json!(version);
    view["registriesManagedBy"] =
        serde_json::json!(state.inner.config.registries_managed_by.as_str());
    view
}

/// Replace the lists `changes` names in `registry`'s rules.
fn apply_rule_changes(
    document: &mut RegistryGraphDocument,
    registry: &str,
    ecosystem: Ecosystem,
    changes: &RuleChanges,
) -> Result<(), RegistryError> {
    let mut lists = document.rule_lists(registry).ok_or(RegistryError::NotFound)?;
    if let Some(access) = &changes.access {
        lists.access = Some(access.clone());
    }
    for (key, package) in &changes.packages {
        let key = declared_key(&lists, registry, ecosystem, key)?;
        let entry = lists.packages.get_mut(&key).ok_or(RegistryError::NotFound)?;
        for (field, change) in [
            (&mut entry.access, &package.access),
            (&mut entry.publish, &package.publish),
            (&mut entry.unpublish, &package.unpublish),
        ] {
            if let Some(change) = change {
                *field = Some(change.clone());
            }
        }
    }
    document.set_rule_lists(registry, lists)
}

/// The `packages:` key `key` names. A key matches when it is written the same
/// way or parses to the same pattern, so the keys this API returns, which are
/// the patterns as pnpr prints them, address the keys the document declares.
fn declared_key(
    lists: &RuleLists,
    registry: &str,
    ecosystem: Ecosystem,
    key: &str,
) -> Result<String, RegistryError> {
    if lists.packages.contains_key(key) {
        return Ok(key.to_string());
    }
    let undeclared = || RegistryError::BadRequest {
        reason: format!("registry {registry:?} does not declare the pattern {key:?}"),
    };
    let wanted = PackagePattern::parse(key, ecosystem).map_err(|_| undeclared())?;
    lists.packages
        .keys()
        .find(|declared| PackagePattern::parse(declared, ecosystem).ok().as_ref() == Some(&wanted))
        .cloned()
        .ok_or_else(undeclared)
}

/// The registry's current rules in the shape the admin API returns.
fn rules_view(table: &RuleTable) -> serde_json::Value {
    let lists = |list: Option<&AccessList>| list.map(AccessList::token_strings);
    let packages: serde_json::Map<String, serde_json::Value> = table
        .rules()
        .iter()
        .map(|rule| {
            let value = serde_json::json!({
                "access": lists(rule.access.as_ref()),
                "publish": lists(rule.publish.as_ref()),
                "unpublish": lists(rule.unpublish.as_ref()),
            });
            (rule.pattern.to_string(), value)
        })
        .collect();
    serde_json::json!({ "access": table.default_access().token_strings(), "packages": packages })
}

/// The hosted registry `path` names, with its key.
fn hosted_registry<'a>(
    routing: &'a RoutingConfig,
    path: &RegistryPath,
) -> Result<(&'a str, &'a HostedConfig), RegistryError> {
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
