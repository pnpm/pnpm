//! npm's team package access endpoints (`npm access grant`, `revoke`, and
//! `list packages <scope:team>`), mapped onto grants.
//!
//! A grant of a package to a team is one grant of the instance: it covers
//! that package in the registry that serves the scope and gives the team
//! `read` and `cache`, and for `read-write` also `publish` and `annotate`.
//! Revoking removes that grant. Other grants that name the team stay.

use super::{
    AppState, Identity, RegistryError, Response, StatusCode,
    grant_directory::{reload_written_grants, written},
    json,
    organizations::{team_registry, visible_teams},
    parse_json_body,
    request_access::require_access_admin,
    team_directory::team_id,
    team_mutations::{TeamScope, respond},
};
use axum::body::Bytes;
use pnpr_policy::Action;
use pnpr_storage::database::GrantContent;
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

impl Permissions {
    const fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::ReadWrite => "read-write",
        }
    }

    fn actions(self) -> &'static [Action] {
        match self {
            Self::ReadOnly => &[Action::Read, Action::Cache],
            Self::ReadWrite => &[Action::Read, Action::Cache, Action::Publish, Action::Annotate],
        }
    }
}

/// The body of `npm access revoke`.
#[derive(Deserialize)]
struct Revoke {
    package: String,
}

/// The name of the grant that gives the team `team_id` the package `package`
/// of `registry`.
fn grant_name(registry: &str, team_id: &str, package: &str) -> String {
    format!("{registry}:team:{team_id}:{package}")
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
        let registry = managed_registry(state, identity, &target, team, "grant access to")?;
        let team_id = team_id(state, team).await?;
        let grant: Grant = parse_json_body(body)?;
        let name = grant_name(&registry, &team_id, &grant.package);
        let subject = format!("team:{team_id}");
        let content = GrantContent {
            name: name.clone(),
            registries: vec![registry.clone()],
            include: vec![grant.package.clone()],
            exclude: Vec::new(),
            subjects: grant.permissions
                .actions()
                .iter()
                .map(|action| (subject.clone(), action.as_str().to_string()))
                .collect(),
        };
        super::grant_directory::validate(state, &content)?;
        let id = existing_id(state, &name).unwrap_or_else(pnpr_auth::new_principal_id);
        written(state.inner.storage.put_grant(&id, content, None).await?, None)?;
        reload_written_grants(state).await?;
        let actor = crate::audit::Actor::of(identity);
        crate::audit!(
            actor,
            "team.access_grant",
            registry = %registry,
            scope = %target.scope,
            team = team,
            package = %grant.package,
            permissions = grant.permissions.label(),
            "team package access granted",
        );
        Ok(StatusCode::CREATED)
    };
    respond(result.await)
}

/// `DELETE /-/team/{scope}/{team}/package`. Revoking what was not granted
/// succeeds, so a retried request does not fail.
pub(super) async fn revoke_team_access(
    state: &AppState,
    identity: &Identity,
    target: TeamScope<'_>,
    team: &str,
    body: &Bytes,
) -> Response {
    let result = async {
        let registry = managed_registry(state, identity, &target, team, "revoke access from")?;
        let team_id = team_id(state, team).await?;
        let revoke: Revoke = parse_json_body(body)?;
        reload_written_grants(state).await?;
        if let Some(id) = existing_id(state, &grant_name(&registry, &team_id, &revoke.package)) {
            state.inner.storage.remove_grant(&id, None).await?;
            reload_written_grants(state).await?;
        }
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

/// `GET /-/team/{scope}/{team}/package` — the package patterns that grants
/// give the team in `@{scope}`'s registry, as `{"<pattern>": "read-only" |
/// "read-write"}`. A grant that covers every package is listed as `**`.
pub(super) fn list_team_packages(
    state: &AppState,
    identity: &Identity,
    target: &TeamScope<'_>,
    team: &str,
) -> Response {
    let result = (|| {
        let teams = visible_teams(state, identity, target.registry, target.scope)?;
        let (id, _) = teams.named(team)?;
        let routing = state.routing();
        let (registry, _) =
            team_registry(state, &routing, identity, target.registry, target.scope)?;
        let subject = format!("team:{id}");
        let grants = state.inner.identity.managed.grants.snapshot();
        let mut packages = serde_json::Map::new();
        let covering = grants.grants
            .iter()
            .filter(|grant| {
                grant.content.registries
                    .iter()
                    .any(|named| {
                        named == registry || named == super::grant_directory::EVERY_REGISTRY
                    })
            });
        for grant in covering {
            let gives = |action: Action| {
                grant.content.subjects
                    .iter()
                    .any(|(given, name)| *given == subject && name == action.as_str())
            };
            if !gives(Action::Read) {
                continue;
            }
            let permission = if gives(Action::Publish) { "read-write" } else { "read-only" };
            let all = ["**".to_string()];
            let patterns =
                if grant.content.include.is_empty() { &all[..] } else { &grant.content.include };
            for pattern in patterns {
                let known = packages.get(pattern).and_then(serde_json::Value::as_str);
                if known != Some("read-write") {
                    packages.insert(pattern.clone(), json!(permission));
                }
            }
        }
        Ok((StatusCode::OK, axum::Json(json!(packages))))
    })();
    respond(result)
}

/// The ID of the grant named `name`, in this replica's copy of the grants.
fn existing_id(state: &AppState, name: &str) -> Option<String> {
    state.inner.identity.managed.grants
        .snapshot()
        .named(name)
        .map(|grant| grant.id.clone())
}

/// The registry `target` addresses, whose packages an admin may grant to
/// `team`.
fn managed_registry(
    state: &AppState,
    identity: &Identity,
    target: &TeamScope<'_>,
    team: &str,
    action: &'static str,
) -> Result<String, RegistryError> {
    let routing = state.routing();
    let (registry, _) = team_registry(state, &routing, identity, target.registry, target.scope)?;
    let scope = target.scope.strip_prefix('@').unwrap_or(target.scope);
    require_access_admin(state, identity, action, format!("@{scope}:{team}"))?;
    Ok(registry.to_string())
}
