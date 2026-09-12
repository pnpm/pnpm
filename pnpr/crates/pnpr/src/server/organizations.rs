use super::{
    Action, AppState, Body, CanonicalPackageName, DiscoverySource, Ecosystem, FetchOutcome,
    HostedConfig, HostedGate, Identity, Map, RegistryError, RegistrySource, Response, StatusCode,
    Value, authorize, default_registry_target, discovery_sources, header, hosted_gate,
    hosted_storage, json, not_found, resolve_registry_source,
};
use axum::response::IntoResponse;

/// `GET /-/org/{scope}/package` — the npm-compatible package-permission map.
/// Values are useful to npm's account UI; registry UIs such as npmX consume
/// the keys as the authoritative package list.
pub(super) async fn serve_org_packages(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_scope: &str,
) -> Response {
    let scope = raw_scope.strip_prefix('@').unwrap_or(raw_scope);
    if CanonicalPackageName::parse(&format!("@{scope}/package"), pnpr_package_name::Ecosystem::Npm)
        .is_err()
    {
        return not_found();
    }
    let Some(registry) =
        registry.map(str::to_string).or_else(|| default_registry_target(state, Ecosystem::Npm))
    else {
        return not_found();
    };
    let prefix = format!("@{scope}/");
    let mut packages = Map::new();
    for source in discovery_sources(state, &registry, Ecosystem::Npm) {
        let scanned = match source {
            DiscoverySource::Hosted(source) => {
                let scan = OrgScan { registry: &registry, source: &source, prefix: &prefix };
                add_hosted_org_packages(state, identity, scan, &mut packages).await
            }
            DiscoverySource::Upstream(source) => {
                let scan = OrgScan { registry: &registry, source: &source, prefix: &prefix };
                add_upstream_org_packages(state, identity, scan, scope, &mut packages).await
            }
        };
        if let Err(err) = scanned {
            return err.into_response();
        }
    }
    if packages.is_empty() {
        return not_found();
    }
    match serde_json::to_vec(&packages) {
        Ok(body) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .expect("static-shape response always builds"),
        Err(err) => RegistryError::Json(err).into_response(),
    }
}

/// One source of an org package scan.
pub(super) struct OrgScan<'a> {
    pub(super) registry: &'a str,
    pub(super) source: &'a str,
    /// The `@scope/` the listing is restricted to.
    pub(super) prefix: &'a str,
}

/// Add the scope's hosted packages, each with the permission this caller has
/// on it.
pub(super) async fn add_hosted_org_packages(
    state: &AppState,
    identity: &Identity,
    scan: OrgScan<'_>,
    packages: &mut Map<String, Value>,
) -> Result<(), RegistryError> {
    let Some(hosted) = state.inner.config.hosted.get(scan.source) else {
        return Ok(());
    };
    if !hosted.rules.any_access_admits(identity) {
        return Ok(());
    }
    let storage = hosted_storage(state, Some(&hosted.org));
    let mut names = storage.hosted_package_names().await?;
    names.sort();
    let resolved = RegistrySource::Hosted(scan.source.to_string());
    for name in names {
        if !hosted_org_package_is_visible(state, identity, &scan, &name) {
            continue;
        }
        let writable = authorize(state, identity, &resolved, &name, Action::Publish).is_ok();
        let permission = if writable { "write" } else { "read" };
        packages.insert(name, Value::String(permission.to_string()));
    }
    Ok(())
}

/// Whether one hosted name belongs to the scanned scope and source, and this
/// caller may read it.
pub(super) fn hosted_org_package_is_visible(
    state: &AppState,
    identity: &Identity,
    scan: &OrgScan<'_>,
    name: &str,
) -> bool {
    name.starts_with(scan.prefix)
        && matches!(
            resolve_registry_source(state, scan.registry, name),
            RegistrySource::Hosted(candidate) if candidate == scan.source,
        )
        && matches!(hosted_gate(state, identity, scan.source, name), HostedGate::Allowed(_))
}

/// Add the scope's upstream packages, which are always read-only here.
pub(super) async fn add_upstream_org_packages(
    state: &AppState,
    identity: &Identity,
    scan: OrgScan<'_>,
    scope: &str,
    packages: &mut Map<String, Value>,
) -> Result<(), RegistryError> {
    let Some(config) = state.inner.config.upstreams.get(scan.source) else {
        return Ok(());
    };
    if !config.search || config.access.as_ref().is_some_and(|access| !access.allows(identity)) {
        return Ok(());
    }
    let Some(upstream) = state.inner.upstreams.get(scan.source) else {
        return Ok(());
    };
    let upstream_packages = match upstream.fetch_org_packages(scope).await? {
        FetchOutcome::Ok(packages) => packages,
        FetchOutcome::NotFound => return Ok(()),
    };
    let resolved = RegistrySource::Upstream(scan.source.to_string());
    for (name, _) in upstream_packages {
        if !upstream_org_package_is_visible(state, identity, &scan, &resolved, &name) {
            continue;
        }
        packages.entry(name).or_insert_with(|| Value::String("read".to_string()));
    }
    Ok(())
}

/// Whether one upstream name belongs to the scanned scope and source, and this
/// caller may read it.
pub(super) fn upstream_org_package_is_visible(
    state: &AppState,
    identity: &Identity,
    scan: &OrgScan<'_>,
    resolved: &RegistrySource,
    name: &str,
) -> bool {
    name.starts_with(scan.prefix)
        && matches!(
            resolve_registry_source(state, scan.registry, name),
            RegistrySource::Upstream(candidate) if candidate == scan.source,
        )
        && authorize(state, identity, resolved, name, Action::Access).is_ok()
}

// --------------------------------------------------------------------
// npm team API — read-only views over the config-declared `teams:` maps.
// Team membership is part of the registry configuration (it feeds the
// compiled access lists), so the API serves listings and rejects
// mutations with an explicit "config-managed" error.
// --------------------------------------------------------------------

/// The hosted registry whose teams `@{scope}` addresses: the scope routes
/// through the addressed registry (an explicit `/~<name>/`, or the
/// path-less default) exactly as a package read in that scope would, then
/// the registry-level default `access` gates the caller. A denial is
/// masked as not-found — team and member names must not become an
/// existence probe for a private registry.
pub(super) fn team_registry<'a>(
    state: &'a AppState,
    identity: &Identity,
    registry: Option<&str>,
    scope: &str,
) -> Result<&'a HostedConfig, RegistryError> {
    let scope = scope.strip_prefix('@').unwrap_or(scope);
    if scope.is_empty() {
        return Err(RegistryError::NotFound);
    }
    let target = match registry {
        Some(registry) => registry.to_string(),
        None => match default_registry_target(state, Ecosystem::Npm) {
            Some(target) => target,
            None => return Err(RegistryError::NotFound),
        },
    };
    let probe = format!("@{scope}/-");
    let RegistrySource::Hosted(source) = resolve_registry_source(state, &target, &probe) else {
        return Err(RegistryError::NotFound);
    };
    let Some(hosted) = state.inner.config.hosted.get(&source) else {
        return Err(RegistryError::NotFound);
    };
    if !hosted.rules.default_access().allows(identity) {
        return Err(RegistryError::NotFound);
    }
    Ok(hosted)
}

/// `GET /-/org/{scope}/team` (path-less) or `GET /~<name>/-/org/{scope}/team`
/// — list the teams of the hosted registry that claims `@{scope}`, in the
/// shape the pnpm team command consumes: an array of `{"name": ...}`.
pub(super) fn get_org_teams(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    scope: &str,
) -> Response {
    let hosted = match team_registry(state, identity, registry, scope) {
        Ok(hosted) => hosted,
        Err(err) => return err.into_response(),
    };
    let teams: Vec<Value> = hosted.teams.keys().map(|name| json!({ "name": name })).collect();
    (StatusCode::OK, axum::Json(Value::Array(teams))).into_response()
}

/// `GET /-/team/{scope}/{team}/user` (path-less) or
/// `GET /~<name>/-/team/{scope}/{team}/user` — list a team's members, in
/// the shape the pnpm team command consumes: an array of `{"name": ...}`.
pub(super) fn get_team_members(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    scope: &str,
    team: &str,
) -> Response {
    let hosted = match team_registry(state, identity, registry, scope) {
        Ok(hosted) => hosted,
        Err(err) => return err.into_response(),
    };
    let Some(members) = hosted.teams.get(team) else {
        return not_found();
    };
    let members: Vec<Value> = members.iter().map(|name| json!({ "name": name })).collect();
    (StatusCode::OK, axum::Json(Value::Array(members))).into_response()
}

/// Every team mutation — create (`PUT /-/org/{scope}/team`), destroy
/// (`DELETE /-/team/{scope}/{team}`), member add/remove
/// (`PUT`/`DELETE /-/team/{scope}/{team}/user`) — answers 403: pnpr teams
/// are declared in the registry config. The same gate as the reads runs
/// first, so a caller who may not see the registry keeps the not-found
/// mask.
pub(super) fn reject_team_mutation(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    scope: &str,
    action: &'static str,
) -> Response {
    if let Err(response) = team_registry(state, identity, registry, scope) {
        return response.into_response();
    }
    RegistryError::TeamsConfigManaged { action }.into_response()
}
