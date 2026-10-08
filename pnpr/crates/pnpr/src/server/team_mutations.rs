use super::{
    AppState, Identity, RegistryError, Response, StatusCode, json, organizations::team_registry,
    require_admin, team_rosters::update_team_roster,
};
use axum::{body::Bytes, response::IntoResponse};
use pnpr_config::{Management, Teams, validate_member_name, validate_team_name};
use serde::Deserialize;
use std::collections::BTreeSet;

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

/// `PUT /-/org/{scope}/team`.
pub(super) async fn create_team(
    state: &AppState,
    identity: &Identity,
    target: TeamScope<'_>,
    body: &Bytes,
) -> Response {
    let result = async {
        let registry = admin_roster(state, identity, &target, "create a team")?;
        let team = parse_body::<NewTeam>(body)?.name;
        validate_team_name(&team).map_err(|reason| RegistryError::BadRequest { reason })?;
        update_team_roster(state, registry, |teams| insert_team(teams, &team)).await?;
        Ok((StatusCode::CREATED, axum::Json(json!({ "name": team }))))
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
        if state.inner.config.routing.hosted[registry].rules.references_team(team) {
            return Err(RegistryError::AdminConflict {
                reason: format!(
                    "team {team:?} is named by this registry's `packages:` rules; remove it \
                     from the configuration first",
                ),
            });
        }
        update_team_roster(state, registry, |teams| {
            teams
                .shift_remove(team)
                .map(drop)
                .ok_or(RegistryError::NotFound)
        })
        .await?;
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
        let user = parse_body::<TeamMember>(body)?.user;
        validate_member_name(&user).map_err(|reason| RegistryError::BadRequest { reason })?;
        update_team_roster(state, registry, |teams| {
            team_members(teams, team)?.insert(user.clone());
            Ok(())
        })
        .await?;
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
        let user = parse_body::<TeamMember>(body)?.user;
        update_team_roster(state, registry, |teams| {
            team_members(teams, team)?.remove(&user);
            Ok(())
        })
        .await?;
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
    require_admin(state, identity, action, format!("on @{scope}"))?;
    Ok(registry)
}

fn insert_team(teams: &mut Teams, team: &str) -> Result<(), RegistryError> {
    if teams.contains_key(team) {
        return Err(RegistryError::AdminConflict {
            reason: format!("team {team:?} already exists"),
        });
    }
    teams.insert(team.to_string(), BTreeSet::default());
    Ok(())
}

fn team_members<'a>(
    teams: &'a mut Teams,
    team: &str,
) -> Result<&'a mut BTreeSet<String>, RegistryError> {
    teams.get_mut(team).ok_or(RegistryError::NotFound)
}

fn parse_body<'a, Body: Deserialize<'a>>(body: &'a Bytes) -> Result<Body, RegistryError> {
    serde_json::from_slice(body)
        .map_err(|err| RegistryError::BadRequest { reason: format!("invalid request body: {err}") })
}

pub(super) fn respond(result: Result<impl IntoResponse, RegistryError>) -> Response {
    match result {
        Ok(response) => response.into_response(),
        Err(err) => err.into_response(),
    }
}
