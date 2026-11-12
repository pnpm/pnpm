//! npm's team package access endpoints (`npm access grant`, `revoke`, and
//! `list packages <scope:team>`), mapped onto the `packages:` rules of a
//! hosted registry in the stored registry graph.
//!
//! A grant edits the rule of the package's own pattern: `read-write` puts
//! `team:<team>` in its `access` and `publish` lists, `read-only` in `access`
//! only. Revoking takes the team out of all three lists. A package without a
//! pattern of its own is refused, because editing the scope's pattern would
//! grant the team every package in the scope.

use super::{
    AppState, Identity, RegistryError, Response, StatusCode, json,
    organizations::team_registry,
    parse_json_body,
    registry_graph::{GraphEdit, edit_graph},
    request_access::require_access_admin,
    team_mutations::{TeamScope, respond},
};
use axum::body::Bytes;
use pnpr_config::{HostedConfig, Management, RegistryGraphDocument, RuleLists};
use pnpr_policy::{PackageRule, RuleTable};
use pnpr_registry::{Ecosystem, PackagePattern};
use serde::Deserialize;

/// The body of `npm access grant`.
#[derive(Deserialize)]
struct Grant {
    package: String,
    permissions: Permissions,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Permissions {
    ReadOnly,
    ReadWrite,
}

/// The body of `npm access revoke`.
#[derive(Deserialize)]
struct Revoke {
    package: String,
}

/// `PUT /-/team/{scope}/{team}/package`.
pub(super) async fn grant_team_access(
    state: &AppState,
    identity: &Identity,
    target: TeamScope<'_>,
    team: &str,
    body: &Bytes,
) -> Response {
    let result = async {
        let registry = managed_team(state, identity, &target, team, "grant access to")?;
        let grant: Grant = parse_json_body(body)?;
        let id = super::team_mutations::team_id(state, &registry, team)?;
        let token = format!("team:{id}");
        let defaults = list_defaults(state, &registry)?;
        edit_graph(state, GraphEdit::latest(crate::audit::Actor::of(identity)), |document| {
            edit_team_lists(document, &registry, &defaults, &grant.package, |lists| {
                grant_lists(lists, &token, grant.permissions);
            })
        })
        .await?;
        let perm_str = match grant.permissions {
            Permissions::ReadOnly => "read-only",
            Permissions::ReadWrite => "read-write",
        };
        let actor = crate::audit::Actor::of(identity);
        crate::audit!(
            actor,
            "team.access_grant",
            registry = %registry,
            scope = %target.scope,
            team = team,
            package = %grant.package,
            permissions = perm_str,
            "team package access granted",
        );
        Ok(StatusCode::CREATED)
    };
    respond(result.await)
}

/// `DELETE /-/team/{scope}/{team}/package`.
pub(super) async fn revoke_team_access(
    state: &AppState,
    identity: &Identity,
    target: TeamScope<'_>,
    team: &str,
    body: &Bytes,
) -> Response {
    let result = async {
        let registry = managed_team(state, identity, &target, team, "revoke access from")?;
        let revoke: Revoke = parse_json_body(body)?;
        let id = super::team_mutations::team_id(state, &registry, team)?;
        let token = format!("team:{id}");
        let defaults = list_defaults(state, &registry)?;
        edit_graph(state, GraphEdit::latest(crate::audit::Actor::of(identity)), |document| {
            edit_team_lists(document, &registry, &defaults, &revoke.package, |lists| {
                for list in lists {
                    remove(list, &token);
                }
            })
        })
        .await?;
        let actor = crate::audit::Actor::of(identity);
        crate::audit!(
            actor,
            "team.access_revoke",
            registry = %registry,
            scope = %target.scope,
            team = team,
            package = %revoke.package,
            "team package access revoked",
        );
        Ok(StatusCode::NO_CONTENT)
    };
    respond(result.await)
}

/// `GET /-/team/{scope}/{team}/package` — the declared patterns whose rules
/// name the team and that the caller may read, as `{"<pattern>": "read-only"
/// | "read-write"}`.
pub(super) fn list_team_packages(
    state: &AppState,
    identity: &Identity,
    target: &TeamScope<'_>,
    team: &str,
) -> Response {
    let result = (|| {
        let routing = state.routing();
        let (registry, hosted) = super::organizations::readable_team_registry(
            state,
            &routing,
            identity,
            target.registry,
            target.scope,
        )?;
        let id = super::team_mutations::team_id(state, registry, team)?;
        let table = hosted.rules.snapshot();
        let packages: serde_json::Map<_, _> = table
            .rules()
            .iter()
            .filter_map(|rule| team_permission(&table, rule, &id, identity))
            .collect();
        Ok((StatusCode::OK, axum::Json(json!(packages))))
    })();
    respond(result)
}

fn team_permission(
    table: &RuleTable,
    rule: &PackageRule,
    team: &str,
    identity: &Identity,
) -> Option<(String, serde_json::Value)> {
    let access = rule.access.as_ref().unwrap_or_else(|| table.default_access());
    if !access.references_team(team) || !access.allows(identity) {
        return None;
    }
    let publish = rule.publish.as_ref().unwrap_or_else(|| table.default_publish());
    let permission = if publish.references_team(team) { "read-write" } else { "read-only" };
    Some((rule.pattern.to_string(), json!(permission)))
}

/// The registry whose rules an admin may edit for `team`.
fn managed_team(
    state: &AppState,
    identity: &Identity,
    target: &TeamScope<'_>,
    team: &str,
    action: &'static str,
) -> Result<String, RegistryError> {
    let routing = state.routing();
    let (registry, hosted) =
        team_registry(state, &routing, identity, target.registry, target.scope)?;
    if state.inner.config.registries_managed_by == Management::Config {
        return Err(RegistryError::RulesConfigManaged);
    }
    let scope = target.scope.strip_prefix('@').unwrap_or(target.scope);
    require_access_admin(state, identity, action, format!("@{scope}:{team}"))?;
    if !holds_team(hosted, team) {
        return Err(RegistryError::NotFound);
    }
    Ok(registry.to_string())
}

fn holds_team(hosted: &HostedConfig, team: &str) -> bool {
    hosted.teams
        .snapshot()
        .values()
        .any(|record| record.name == team)
}

/// The registry-level `access`, `publish`, and `unpublish` lists an omitted
/// list of `registry` falls back to.
fn list_defaults(state: &AppState, registry: &str) -> Result<[Vec<String>; 3], RegistryError> {
    let routing = state.routing();
    let hosted = routing.hosted.get(registry).ok_or(RegistryError::NotFound)?;
    let table: std::sync::Arc<RuleTable> = hosted.rules.snapshot();
    Ok([
        table.default_access().token_strings(),
        table.default_publish().token_strings(),
        table.default_unpublish().token_strings(),
    ])
}

/// Apply `edit` to the `access`, `publish`, and `unpublish` lists that apply
/// to `package`'s own pattern now, and store each list the edit changed.
/// Lists it left alone keep falling back to the registry defaults.
fn edit_team_lists(
    document: &mut RegistryGraphDocument,
    registry: &str,
    defaults: &[Vec<String>; 3],
    package: &str,
    edit: impl FnOnce(&mut [Vec<String>; 3]),
) -> Result<(), RegistryError> {
    let mut lists = document.rule_lists(registry).ok_or(RegistryError::NotFound)?;
    let key = own_key(&lists, package)?;
    let access_default = lists.access
        .clone()
        .unwrap_or_else(|| defaults[0].clone());
    let defaults = [access_default, defaults[1].clone(), defaults[2].clone()];
    let entry = lists.packages.get_mut(&key).ok_or(RegistryError::NotFound)?;
    let fields = [&mut entry.access, &mut entry.publish, &mut entry.unpublish];
    let before: [Vec<String>; 3] = std::array::from_fn(|index| {
        fields[index]
            .clone()
            .unwrap_or_else(|| defaults[index].clone())
    });
    let mut after = before.clone();
    edit(&mut after);
    for ((field, before), after) in fields
        .into_iter()
        .zip(before)
        .zip(after)
    {
        if after != before {
            *field = Some(after);
        }
    }
    document.set_rule_lists(registry, lists)
}

/// The `packages:` key that names `package` exactly. A pattern covering other
/// packages too is refused.
fn own_key(lists: &RuleLists, package: &str) -> Result<String, RegistryError> {
    lists.packages
        .keys()
        .find(|key| {
            matches!(
                PackagePattern::parse(key, Ecosystem::Npm),
                Ok(PackagePattern::Exact(name)) if name == package,
            )
        })
        .cloned()
        .ok_or_else(|| RegistryError::BadRequest {
            reason: format!(
                "{package:?} has no rule of its own in this registry's `packages:` map; pnpr grants \
                 a team access per declared package, so declare it there first",
            ),
        })
}

fn grant_lists([access, publish, _]: &mut [Vec<String>; 3], token: &str, permissions: Permissions) {
    add(access, token);
    match permissions {
        Permissions::ReadWrite => add(publish, token),
        Permissions::ReadOnly => remove(publish, token),
    }
}

fn remove(list: &mut Vec<String>, token: &str) {
    list.retain(|entry| entry != token);
}

fn add(list: &mut Vec<String>, token: &str) {
    if !list.iter().any(|entry| entry == token) {
        list.push(token.to_string());
    }
}
