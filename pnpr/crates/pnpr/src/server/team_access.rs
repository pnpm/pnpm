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
use pnpr_registry::{Ecosystem, PackagePattern};
use pnpr_storage::database::{GrantContent, GrantWrite, StoredGrant};
use serde::Deserialize;
use std::collections::BTreeMap;

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
        let mut stored = put_named(state, &name, content.clone()).await?;
        if stored == GrantWrite::NameTaken {
            // Another replica created the grant since the reload.
            stored = put_named(state, &name, content).await?;
        }
        written(stored, None)?;
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

/// Store `content` under the ID the grant named `name` has, as of a reload,
/// or under a new one.
async fn put_named(
    state: &AppState,
    name: &str,
    content: GrantContent,
) -> Result<GrantWrite, RegistryError> {
    reload_written_grants(state).await?;
    let id = existing_id(state, name).unwrap_or_else(pnpr_auth::new_principal_id);
    state.inner.storage.put_grant(&id, content, None).await
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
        let grants = state.inner.identity.managed.grants.snapshot();
        let ecosystem = routing.registries.ecosystem(registry).unwrap_or_default();
        let packages = team_packages(&grants.grants, (registry, ecosystem), id);
        Ok((StatusCode::OK, axum::Json(json!(packages))))
    })();
    respond(result)
}

/// The patterns `grants` give the team `id` in `registry`, each with what the
/// grants covering it give between them. A pattern a grant excludes in full
/// is not counted for that grant. A pattern a grant excludes part of is
/// listed, which npm's listing cannot qualify.
fn team_packages(
    grants: &[StoredGrant],
    (registry, ecosystem): (&str, Ecosystem),
    id: &str,
) -> serde_json::Map<String, serde_json::Value> {
    let scopes: Vec<TeamScopeOfGrant> = grants
        .iter()
        .map(|grant| &grant.content)
        .filter(|content| covers(content, registry))
        .map(|content| TeamScopeOfGrant::of(content, ecosystem, id))
        .filter(|scope| scope.actions.read || scope.actions.publish)
        .collect();
    let listed: BTreeMap<String, &PackagePattern> = scopes
        .iter()
        .flat_map(TeamScopeOfGrant::listed)
        .map(|pattern| (pattern.to_string(), pattern))
        .collect();
    listed
        .into_iter()
        .filter_map(|(name, pattern)| {
            let mut actions = TeamActions::default();
            for scope in scopes
                .iter()
                .filter(|scope| scope.gives_all_of(pattern))
            {
                actions.add(scope.actions);
            }
            Some((name, json!(actions.label()?)))
        })
        .collect()
}

/// What one grant gives a team, and on which packages.
struct TeamScopeOfGrant {
    include: Vec<PackagePattern>,
    exclude: Vec<PackagePattern>,
    actions: TeamActions,
}

impl TeamScopeOfGrant {
    fn of(content: &GrantContent, ecosystem: Ecosystem, team: &str) -> Self {
        let parse = |patterns: &[String]| -> Vec<PackagePattern> {
            patterns
                .iter()
                .filter_map(|pattern| PackagePattern::parse(pattern, ecosystem).ok())
                .collect()
        };
        let mut include = parse(&content.include);
        if content.include.is_empty() {
            include.push(PackagePattern::All);
        }
        Self { include, exclude: parse(&content.exclude), actions: TeamActions::of(content, team) }
    }

    /// The patterns the grant includes and does not exclude in full.
    fn listed(&self) -> impl Iterator<Item = &PackagePattern> {
        self.include
            .iter()
            .filter(|pattern| self.gives_all_of(pattern))
    }

    /// Whether the grant includes all of `pattern` and does not exclude all
    /// of it.
    fn gives_all_of(&self, pattern: &PackagePattern) -> bool {
        self.include
            .iter()
            .any(|include| include.covers(pattern))
            && !self.exclude
                .iter()
                .any(|exclude| exclude.covers(pattern))
    }
}

/// What grants let a team do with a pattern, in the actions npm's listing
/// tells apart.
#[derive(Debug, Default, Clone, Copy)]
struct TeamActions {
    read: bool,
    publish: bool,
}

impl TeamActions {
    fn of(content: &GrantContent, team: &str) -> Self {
        let subject = format!("team:{team}");
        let gives = |action: Action| {
            content.subjects
                .iter()
                .any(|(given, name)| *given == subject && name == action.as_str())
        };
        Self { read: gives(Action::Read), publish: gives(Action::Publish) }
    }

    fn add(&mut self, other: Self) {
        self.read |= other.read;
        self.publish |= other.publish;
    }

    /// npm's word for the actions, or `None` when they do not let the team read.
    fn label(self) -> Option<&'static str> {
        self.read.then_some(if self.publish { "read-write" } else { "read-only" })
    }
}

/// Whether the grant `content` covers `registry`.
fn covers(content: &GrantContent, registry: &str) -> bool {
    content.registries
        .iter()
        .any(|named| named == registry || named == super::grant_directory::EVERY_REGISTRY)
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
