use super::{
    AppState, Identity, RegistryError, Response, StatusCode, json,
    organizations::team_registry,
    parse_json_body, require_admin, require_caller,
    team_rosters::{
        add_member_to_team, create_team_in_roster, destroy_team_in_roster, remove_member_from_team,
    },
};
use axum::{body::Bytes, response::IntoResponse};
use pnpr_config::{Management, validate_member_name, validate_team_name};
use serde::Deserialize;

/// The registry and scope a team mutation addresses.
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
        let registry = admin_roster(state, identity, &target, "create a team")?;
        let team = parse_json_body::<NewTeam>(body)?.name;
        validate_team_name(&team).map_err(|reason| RegistryError::BadRequest { reason })?;
        let id = pnpr_auth::new_principal_id();
        create_team_in_roster(state, registry, &id, &team).await?;
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

/// `DELETE /-/team/{scope}/{team}`. A team a `packages:` rule names stays,
/// so a rule never loses the team it grants to.
pub(super) async fn destroy_team(
    state: &AppState,
    identity: &Identity,
    target: TeamScope<'_>,
    team: &str,
) -> Response {
    let result = async {
        let registry = admin_roster(state, identity, &target, "destroy a team")?;
        destroy_team_in_roster(state, registry, team).await?;
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
        let registry = admin_roster(state, identity, &target, "add a team member")?;
        let user = parse_json_body::<TeamMember>(body)?.user;
        validate_member_name(&user).map_err(|reason| RegistryError::BadRequest { reason })?;
        let user_id = state.inner.identity.auth.users
            .account_id(&user)
            .await?
            .ok_or(RegistryError::NotFound)?;
        add_member_to_team(state, registry, team, &user_id).await?;
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
        let registry = admin_roster(state, identity, &target, "remove a team member")?;
        let (user_id, user) = member_removal(state, body).await?;
        remove_member_from_team(state, registry, team, &user_id).await?;
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

/// The name of the registry whose roster the caller may edit. The read gate
/// runs first, so a caller who may not see the registry keeps the not-found
/// mask.
fn admin_roster<'a>(
    state: &'a AppState,
    identity: &Identity,
    target: &TeamScope<'_>,
    action: &'static str,
) -> Result<&'a str, RegistryError> {
    let (registry, hosted) = team_registry(state, identity, target.registry, target.scope)?;
    if hosted.teams_managed_by == Management::Config {
        return Err(RegistryError::TeamsConfigManaged { action });
    }
    let scope = target.scope.strip_prefix('@').unwrap_or(target.scope);
    require_caller(identity, &format!("@{scope}"))?;
    require_admin(state, identity, action, format!("on @{scope}"))?;
    Ok(registry)
}

pub(super) fn respond(result: Result<impl IntoResponse, RegistryError>) -> Response {
    match result {
        Ok(response) => response.into_response(),
        Err(err) => err.into_response(),
    }
}

pub(super) fn team_id(
    state: &AppState,
    registry: &str,
    name: &str,
) -> Result<String, RegistryError> {
    state.inner.config.routing.hosted[registry].teams
        .snapshot()
        .iter()
        .find(|(_, team)| team.name == name)
        .map(|(id, _)| id.clone())
        .ok_or(RegistryError::NotFound)
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
