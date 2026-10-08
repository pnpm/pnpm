//! npm's team package access endpoints (`npm access grant`, `revoke`, and
//! `list packages <scope:team>`), mapped onto the `packages:` rules of a
//! registry whose rules the admin API manages.
//!
//! A grant edits the rule of the package's own pattern: `read-write` puts
//! `team:<team>` in its `access` and `publish` lists, `read-only` in `access`
//! only. Revoking takes the team out of all three lists. A package without a
//! pattern of its own is refused, because editing the scope's pattern would
//! grant the team every package in the scope.

use super::{
    AppState, Identity, RegistryError, Response, StatusCode, json,
    organizations::team_registry,
    require_admin,
    rule_overrides::{RuleChanges, update_rules},
    team_mutations::{TeamScope, parse_body, respond},
};
use axum::body::Bytes;
use pnpr_config::{HostedConfig, Management};
use pnpr_policy::{AccessList, PackageRule, RuleTable};
use pnpr_registry::PackagePattern;
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
        let grant: Grant = parse_body(body)?;
        let token = format!("team:{team}");
        update_rules(state, registry, |changes, base| {
            edit_team_lists(changes, base, &grant.package, |lists| {
                grant_lists(lists, &token, grant.permissions);
            })
        })
        .await?;
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
        let revoke: Revoke = parse_body(body)?;
        let token = format!("team:{team}");
        update_rules(state, registry, |changes, base| {
            edit_team_lists(changes, base, &revoke.package, |lists| {
                for list in lists {
                    remove(list, &token);
                }
            })
        })
        .await?;
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
        let (_, hosted) = team_registry(state, identity, target.registry, target.scope)?;
        let table = hosted.rules.snapshot();
        let packages: serde_json::Map<_, _> = table
            .rules()
            .iter()
            .filter_map(|rule| team_permission(&table, rule, team, identity))
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
fn managed_team<'a>(
    state: &'a AppState,
    identity: &Identity,
    target: &TeamScope<'_>,
    team: &str,
    action: &'static str,
) -> Result<&'a str, RegistryError> {
    let (registry, hosted) = team_registry(state, identity, target.registry, target.scope)?;
    if hosted.rules_managed_by == Management::Config {
        return Err(RegistryError::RulesConfigManaged);
    }
    let scope = target.scope.strip_prefix('@').unwrap_or(target.scope);
    require_admin(state, identity, action, format!("@{scope}:{team}"))?;
    if !holds_team(hosted, team) {
        return Err(RegistryError::NotFound);
    }
    Ok(registry)
}

fn holds_team(hosted: &HostedConfig, team: &str) -> bool {
    hosted.teams.snapshot().contains_key(team)
}

/// Apply `edit` to the `access`, `publish`, and `unpublish` lists that apply
/// to `package`'s own pattern now, and store each list the edit changed.
/// Lists it left alone stay unstored, so they keep following the YAML.
fn edit_team_lists(
    changes: &mut RuleChanges,
    base: &RuleTable,
    package: &str,
    edit: impl FnOnce(&mut [Vec<String>; 3]),
) -> Result<(), RegistryError> {
    let rule = own_rule(base, package)?;
    let defaults = [
        changes.access
            .clone()
            .unwrap_or_else(|| base.default_access().token_strings()),
        base.default_publish().token_strings(),
        base.default_unpublish().token_strings(),
    ];
    let stored = changes.packages.entry(package.to_string()).or_default();
    let fields = [&mut stored.access, &mut stored.publish, &mut stored.unpublish];
    let declared = [&rule.access, &rule.publish, &rule.unpublish];
    let before: [Vec<String>; 3] = std::array::from_fn(|index| {
        fields[index]
            .clone()
            .unwrap_or_else(|| {
                declared[index]
                    .as_ref()
                    .map_or_else(|| defaults[index].clone(), AccessList::token_strings)
            })
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
    Ok(())
}

/// The rule `packages:` declares for `package` by name. A pattern covering
/// other packages too is refused.
fn own_rule<'a>(base: &'a RuleTable, package: &str) -> Result<&'a PackageRule, RegistryError> {
    base.rules()
        .iter()
        .find(|rule| matches!(&rule.pattern, PackagePattern::Exact(name) if name == package))
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
