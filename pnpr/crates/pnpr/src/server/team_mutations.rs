use super::{
    AppState,
    Identity,
    RegistryError,
    Response,
    StatusCode,
    json,
    organizations::team_registry,
    parse_json_body,
    request_access::require_access_admin,
    require_caller,
    team_directory::{
        reload_written_teams,
        team_id,
    },
};
use axum::{
    body::Bytes,
    response::IntoResponse,
};
use pnpr_config::{
    validate_member_name,
    validate_team_name,
};
use pnpr_storage::database::{
    TeamCreation,
    TeamRemoval,
};
use serde::Deserialize;

/// The registry and scope a team request addresses. Teams belong to the
/// instance; the scope names the registry whose access the caller must have.
pub(super) struct TeamScope<'a> {
    pub(super) registry: Option<&'a str>,
    pub(super) scope: &'a str,
}

/// The body of `npm team create`.
#[derive(Deserialize)]
struct NewTeam {
    name: String,
}

/// The body of `npm team add` and `npm team rm`.
#[derive(Deserialize)]
struct TeamMember {
    user: String,
}

#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum MemberRemoval {
    Id {
        #[serde(rename = "userId")]
        user_id: String,
    },
    Name {
        user: String,
    },
}

/// `PUT /-/org/{scope}/team`.
pub(super) async fn create_team(
    state: &AppState,
    identity: &Identity,
    target: TeamScope<'_>,
    body: &Bytes,
) -> Response {
    let result = async {
        let registry = admin_registry(state, identity, &target, "create a team")?;
        let team = parse_json_body::<NewTeam>(body)?.name;
        validate_team_name(&team).map_err(|reason| RegistryError::BadRequest { reason })?;
        let id = pnpr_auth::new_principal_id();
        if state.inner.storage.create_team(&id, &team).await? == TeamCreation::NameTaken {
            return Err(RegistryError::AdminConflict {
                reason: format!("team {team:?} already exists"),
            });
        }
        reload_written_teams(state).await;
        let actor = crate::audit::Actor::of(identity);
        crate::audit!(
            actor,
            "team.create",
            registry = %registry,
            scope = %target.scope,
            team = %team,
            "team created",
        );
        Ok((StatusCode::CREATED, axum::Json(json!({ "id": id, "name": team }))))
    };
    respond(result.await)
}

/// `DELETE /-/team/{scope}/{team}`. A team a grant names stays, so no grant
/// loses the team it admits.
pub(super) async fn destroy_team(
    state: &AppState,
    identity: &Identity,
    target: TeamScope<'_>,
    team: &str,
) -> Response {
    let result = async {
        let registry = admin_registry(state, identity, &target, "destroy a team")?;
        let id = team_id(state, team).await?;
        let named = || RegistryError::AdminConflict {
            reason: format!("team {team:?} is named by a grant; remove it from the grant first"),
        };
        if is_named_outside_the_stored_graph(state, &id) {
            return Err(named());
        }
        match state.inner.storage.remove_team(&id).await? {
            TeamRemoval::Removed => {}
            TeamRemoval::NotFound => return Err(RegistryError::NotFound),
            TeamRemoval::Named => return Err(named()),
        }
        reload_written_teams(state).await;
        let actor = crate::audit::Actor::of(identity);
        crate::audit!(
            actor,
            "team.destroy",
            registry = %registry,
            scope = %target.scope,
            team = %team,
            "team destroyed",
        );
        Ok((StatusCode::OK, axum::Json(json!({ "name": team }))))
    };
    respond(result.await)
}

/// `PUT /-/team/{scope}/{team}/user`.
pub(super) async fn add_team_member(
    state: &AppState,
    identity: &Identity,
    target: TeamScope<'_>,
    team: &str,
    body: &Bytes,
) -> Response {
    let result = async {
        let registry = admin_registry(state, identity, &target, "add a team member")?;
        let user = parse_json_body::<TeamMember>(body)?.user;
        validate_member_name(&user).map_err(|reason| RegistryError::BadRequest { reason })?;
        let user_id = state.inner.identity.auth.users
            .account_id(&user)
            .await?
            .ok_or(RegistryError::NotFound)?;
        let id = team_id(state, team).await?;
        if !state.inner.storage.add_team_member(&id, &user_id).await? {
            return Err(RegistryError::NotFound);
        }
        reload_written_teams(state).await;
        let actor = crate::audit::Actor::of(identity);
        crate::audit!(
            actor,
            "team.member_add",
            registry = %registry,
            scope = %target.scope,
            team = %team,
            user = %user,
            user_id = %user_id,
            "team member added",
        );
        Ok((StatusCode::CREATED, axum::Json(json!({}))))
    };
    respond(result.await)
}

/// `DELETE /-/team/{scope}/{team}/user`. Removing a user who is not a member
/// succeeds, so a retried request does not fail.
pub(super) async fn remove_team_member(
    state: &AppState,
    identity: &Identity,
    target: TeamScope<'_>,
    team: &str,
    body: &Bytes,
) -> Response {
    let result = async {
        let registry = admin_registry(state, identity, &target, "remove a team member")?;
        let (user_id, user) = member_removal(state, body).await?;
        let id = team_id(state, team).await?;
        if !state.inner.storage.remove_team_member(&id, &user_id).await? {
            return Err(RegistryError::NotFound);
        }
        reload_written_teams(state).await;
        let actor = crate::audit::Actor::of(identity);
        crate::audit!(
            actor,
            "team.member_remove",
            registry = %registry,
            scope = %target.scope,
            team = %team,
            user = %user,
            user_id = %user_id,
            "team member removed",
        );
        Ok(StatusCode::NO_CONTENT)
    };
    respond(result.await)
}

/// The name of the registry the scope addresses, for a caller who may edit
/// teams. The read gate runs first, so a caller who may not see the registry
/// keeps the not-found mask.
fn admin_registry(
    state: &AppState,
    identity: &Identity,
    target: &TeamScope<'_>,
    action: &'static str,
) -> Result<String, RegistryError> {
    let routing = state.routing();
    let (registry, _) = team_registry(state, &routing, identity, target.registry, target.scope)?;
    let scope = target.scope.strip_prefix('@').unwrap_or(target.scope);
    require_caller(identity, &format!("@{scope}"))?;
    require_access_admin(state, identity, action, format!("on @{scope}"))?;
    Ok(registry.to_string())
}

pub(super) fn respond(result: Result<impl IntoResponse, RegistryError>) -> Response {
    match result {
        Ok(response) => response.into_response(),
        Err(err) => err.into_response(),
    }
}

/// Whether something [`pnpr_storage::Storage::remove_team`] does not read
/// names the team `id`: the `artifacts.owners` lists of the config file, and
/// the rules of a graph this server was given in code.
fn is_named_outside_the_stored_graph(state: &AppState, id: &str) -> bool {
    let routing = state.routing();
    let owners = &state.inner.config.features.artifacts.owners;
    owners
        .values()
        .any(|owner| owner.access.references_team(id) || owner.publish.references_team(id))
        || routing.hosted
            .values()
            .any(|hosted| hosted.rules.references_team(id))
        || routing.upstreams
            .values()
            .any(|upstream| upstream.rules.references_team(id))
}

async fn member_removal(state: &AppState, body: &Bytes) -> Result<(String, String), RegistryError> {
    let users = &state.inner.identity.auth.users;
    match parse_json_body::<MemberRemoval>(body)? {
        MemberRemoval::Id { user_id } => {
            let name = users
                .account_name(&user_id)
                .await?
                .unwrap_or_else(|| format!("deleted:{user_id}"));
            Ok((user_id, name))
        }
        MemberRemoval::Name { user } => {
            let id = users.account_id(&user).await?.ok_or(RegistryError::NotFound)?;
            Ok((id, user))
        }
    }
}
