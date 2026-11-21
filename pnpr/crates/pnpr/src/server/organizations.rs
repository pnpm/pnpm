use super::{
    Action,
    AppState,
    Body,
    CanonicalPackageName,
    DiscoverySource,
    Ecosystem,
    FetchOutcome,
    HostedConfig,
    HostedGate,
    Identity,
    Map,
    Ownership,
    RegistryError,
    RegistrySource,
    Response,
    StatusCode,
    Value,
    allows_with_ownership,
    authorize,
    default_registry_target,
    discovery_sources,
    header,
    hosted_gate,
    hosted_storage,
    json,
    needs_package_owners,
    not_found,
    resolve_registry_source,
    team_directory::{
        Team,
        Teams,
    },
};
use axum::response::IntoResponse;
use pnpr_config::RoutingConfig;

/// `GET /-/org/{scope}/package`: the npm-compatible package-permission map.
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
    let routing = state.routing();
    let Some(hosted) = routing.hosted.get(scan.source) else {
        return Ok(());
    };
    if !hosted.rules.any_access_admits(identity) {
        return Ok(());
    }
    let storage = hosted_storage(state, Some(&hosted.namespace));
    let mut names = storage.hosted_package_names().await?;
    names.sort();
    names.retain(|name| hosted_org_package_is_visible(state, identity, &scan, name));
    let rules = hosted.rules.snapshot();
    let owner_checked: Vec<String> = names
        .iter()
        .filter(|name| needs_package_owners(state, identity, &rules, name, Action::Publish))
        .cloned()
        .collect();
    let owners_by_package = storage.read_packages_owners(&owner_checked).await?;
    for name in names {
        let owners = owners_by_package
            .get(&name)
            .map(Vec::as_slice)
            .unwrap_or(&[][..]);
        let ownership = Ownership::Owners(owners);
        let permission =
            if allows_with_ownership(state, identity, &rules, (&name, &ownership), Action::Publish)
            {
                "write"
            } else {
                "read"
            };
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
    let routing = state.routing();
    let Some(config) = routing.upstreams.get(scan.source) else {
        return Ok(());
    };
    // The listing asks the origin, so it takes `cache` as well as `read`.
    let admitted = config.rules.any_access_admits(identity)
        && config.rules.any_admits(pnpr_policy::Action::Cache, identity);
    if !config.search || !admitted {
        return Ok(());
    }
    let Some(upstream) = routing.upstream_clients.get(scan.source) else {
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
        && authorize(state, identity, resolved, name, Action::Read).is_ok()
}

/// The hosted registry `@{scope}` addresses in a team request: the scope routes
/// through the addressed registry (an explicit `/~<name>/`, or the
/// path-less default) exactly as a package read in that scope would, then
/// whether the caller may read anything in that registry gates it. A denial is
/// masked as not-found, team and member names must not become an
/// existence probe for a private registry. Returns the registry's name with
/// its config. Teams belong to the instance, so the registry only decides who
/// may ask.
pub(super) fn team_registry<'a>(
    state: &AppState,
    routing: &'a RoutingConfig,
    identity: &Identity,
    registry: Option<&str>,
    scope: &str,
) -> Result<(&'a str, &'a HostedConfig), RegistryError> {
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
    let Some((name, hosted)) = routing.hosted.get_key_value(&source) else {
        return Err(RegistryError::NotFound);
    };
    if !hosted.rules.any_access_admits(identity) {
        return Err(RegistryError::NotFound);
    }
    Ok((name, hosted))
}

/// The teams a caller who reaches `@{scope}`'s registry may see: every team
/// for an admin, and otherwise the teams the caller is in.
pub(super) struct VisibleTeams {
    teams: std::sync::Arc<Teams>,
    /// `None` for an admin.
    own: Option<Vec<String>>,
}

impl VisibleTeams {
    /// Sorted by name.
    pub(super) fn list(&self) -> Vec<(&str, &Team)> {
        let Some(own) = &self.own else {
            return self.teams.iter().collect();
        };
        let mut teams: Vec<_> = own
            .iter()
            .filter_map(|id| self.teams.get(id))
            .collect();
        teams.sort_by_key(|(_, team)| team.name.as_str());
        teams
    }

    pub(super) fn named(&self, name: &str) -> Result<(&str, &Team), RegistryError> {
        self.list()
            .into_iter()
            .find(|(_, team)| team.name == name)
            .ok_or(RegistryError::NotFound)
    }
}

/// [`VisibleTeams`] for `identity`. A caller who is in no team and is not an
/// admin gets not-found.
pub(super) fn visible_teams(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    scope: &str,
) -> Result<VisibleTeams, RegistryError> {
    team_registry(state, &state.routing(), identity, registry, scope)?;
    super::require_caller(identity, "team listing")?;
    let auth = &state.inner.config.identity;
    let sees_all = auth.is_admin(identity) || auth.is_registry_admin(identity);
    let own = (!sees_all).then(|| identity.teams().to_vec());
    let visible = VisibleTeams { teams: state.inner.identity.managed.teams.snapshot(), own };
    if !sees_all && visible.list().is_empty() {
        return Err(RegistryError::NotFound);
    }
    Ok(visible)
}

/// `GET /-/org/{scope}/team` (path-less) or `GET /~<name>/-/org/{scope}/team`,
/// list the teams the caller may see.
pub(super) fn get_org_teams(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    scope: &str,
    cli: bool,
) -> Response {
    let teams = match visible_teams(state, identity, registry, scope) {
        Ok(teams) => teams,
        Err(err) => return err.into_response(),
    };
    let teams: Vec<Value> = teams
        .list()
        .into_iter()
        .map(|(id, team)| {
            if cli {
                json!(format!("{}:{}", scope.trim_start_matches('@'), team.name))
            } else {
                json!({ "id": id, "name": team.name })
            }
        })
        .collect();
    (StatusCode::OK, axum::Json(Value::Array(teams))).into_response()
}

/// `GET /-/team/{scope}/{team}/user` (path-less) or
/// `GET /~<name>/-/team/{scope}/{team}/user`, list a team's members.
pub(super) async fn get_team_members(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    (scope, team): (&str, &str),
    cli: bool,
) -> Response {
    let teams = match visible_teams(state, identity, registry, scope) {
        Ok(teams) => teams,
        Err(err) => return err.into_response(),
    };
    let record = match teams.named(team) {
        Ok((_, record)) => record,
        Err(err) => return err.into_response(),
    };
    let ids: Vec<String> = record.members.iter().cloned().collect();
    let accounts = match state.inner.identity.auth.users.accounts_by_id(&ids).await {
        Ok(accounts) => accounts,
        Err(err) => return err.into_response(),
    };
    let mut members = build_team_member_list(&record.members, accounts);
    if cli {
        for member in &mut members {
            *member = member["name"].take();
        }
    }
    (StatusCode::OK, axum::Json(Value::Array(members))).into_response()
}

fn build_team_member_list(
    ids: &std::collections::BTreeSet<String>,
    accounts: Vec<pnpr_auth::UserAccount>,
) -> Vec<Value> {
    let names: std::collections::HashMap<_, _> = accounts
        .into_iter()
        .map(|account| (account.id, account.username))
        .collect();
    let mut members: Vec<_> = ids
        .iter()
        .map(|id| {
            let name = names
                .get(id)
                .cloned()
                .unwrap_or_else(|| format!("deleted:{id}"));
            json!({ "id": id, "name": name })
        })
        .collect();
    members.sort_by(|left, right| {
        left["name"]
            .as_str()
            .cmp(&right["name"].as_str())
    });
    members
}
