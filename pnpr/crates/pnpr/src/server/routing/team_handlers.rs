use super::{
    AppState, AuthedCaller, Deserialize, Path, Response, State, TargetRegistry, get_org_teams,
    get_team_members, private_no_cache, reject_team_mutation, serve_org_packages,
};

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
// Orgs and teams. Membership is config-managed, so every mutation is
// rejected with an explanation rather than silently ignored.
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
) -> Response {
    reject_team_mutation(&state, &identity, registry.as_deref(), &path.scope, "create a team")
}

/// `DELETE {base}/-/team/{scope}/{team}`.
pub(super) async fn delete_team(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopePath>,
) -> Response {
    reject_team_mutation(&state, &identity, registry.as_deref(), &path.scope, "destroy a team")
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
    Path(path): Path<ScopePath>,
) -> Response {
    reject_team_mutation(&state, &identity, registry.as_deref(), &path.scope, "add a team member")
}

/// `DELETE {base}/-/team/{scope}/{team}/user`.
pub(super) async fn delete_team_user(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(path): Path<ScopePath>,
) -> Response {
    reject_team_mutation(
        &state,
        &identity,
        registry.as_deref(),
        &path.scope,
        "remove a team member",
    )
}
