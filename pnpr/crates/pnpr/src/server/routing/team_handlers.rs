use super::{
    AppState, AuthedCaller, Deserialize, Path, Response, State, TargetRegistry, TeamScope,
    add_team_member, create_team, destroy_team, get_org_teams, get_team_members, private_no_cache,
    remove_team_member, serve_org_packages,
};
use axum::body::Bytes;

#[derive(Deserialize)]
pub(super) struct ScopePath {
    pub(super) scope: String,
}

#[derive(Deserialize)]
pub(super) struct TeamPath {
    pub(super) scope: String,
    pub(super) team: String,
}

// --------------------------------------------------------------------
// Orgs and teams.
// --------------------------------------------------------------------

/// `GET {base}/-/org/{scope}/team` — the teams of the registry claiming
/// `scope`.
pub(super) async fn get_teams(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopePath>,
) -> Response {
    private_no_cache(get_org_teams(&state, &identity, registry.as_deref(), &path.scope))
}

/// `GET {base}/-/org/{scope}/package` — the packages of the registry claiming
/// `scope`.
pub(super) async fn get_org_package_list(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopePath>,
) -> Response {
    private_no_cache(serve_org_packages(&state, &identity, registry.as_deref(), &path.scope).await)
}

/// `PUT {base}/-/org/{scope}/team`.
pub(super) async fn put_team(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopePath>,
    body: Bytes,
) -> Response {
    let target = TeamScope { registry: registry.as_deref(), scope: &path.scope };
    create_team(&state, &identity, target, &body).await
}

/// `DELETE {base}/-/team/{scope}/{team}`.
pub(super) async fn delete_team(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<TeamPath>,
) -> Response {
    let target = TeamScope { registry: registry.as_deref(), scope: &path.scope };
    destroy_team(&state, &identity, target, &path.team).await
}

/// `GET {base}/-/team/{scope}/{team}/user`.
pub(super) async fn get_team_users(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<TeamPath>,
) -> Response {
    private_no_cache(get_team_members(
        &state,
        &identity,
        registry.as_deref(),
        &path.scope,
        &path.team,
    ))
}

/// `PUT {base}/-/team/{scope}/{team}/user`.
pub(super) async fn put_team_user(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<TeamPath>,
    body: Bytes,
) -> Response {
    let target = TeamScope { registry: registry.as_deref(), scope: &path.scope };
    add_team_member(&state, &identity, target, &path.team, &body).await
}

/// `DELETE {base}/-/team/{scope}/{team}/user`.
pub(super) async fn delete_team_user(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<TeamPath>,
    body: Bytes,
) -> Response {
    let target = TeamScope { registry: registry.as_deref(), scope: &path.scope };
    remove_team_member(&state, &identity, target, &path.team, &body).await
}
