use super::{
    Action, AppState, BTreeMap, Identity, RegistryError, authorize, resolve_ecosystem_source,
};
/// The scope request a token issue carries.
pub(super) struct GrantQuery<'a> {
    pub(super) uri: &'a axum::http::Uri,
    /// Whether the parent token is read-only, which caps every scope at pull.
    pub(super) readonly: bool,
}

/// One `repository:<name>:<actions>` scope of the query.
struct GrantOne<'a> {
    scope: &'a str,
    readonly: bool,
    scopes: &'a mut BTreeMap<String, Vec<String>>,
}

/// The actions of one scope, and what the caller has been granted so far.
struct GrantActions<'a> {
    name: &'a str,
    actions: &'a str,
    readonly: bool,
    allowed: &'a mut Vec<String>,
}

/// One action of one scope.
struct GrantAction<'a> {
    name: &'a str,
    action: &'a str,
    readonly: bool,
}

/// The repository scopes this caller is granted out of the ones the query asks
/// for. A scope naming a repository the caller cannot even read is dropped,
/// not refused: the registry protocol answers an unauthorized pull with an
/// empty grant.
pub(super) fn granted_scopes(
    state: &AppState,
    identity: &Identity,
    target: &str,
    query: &GrantQuery<'_>,
) -> Result<BTreeMap<String, Vec<String>>, RegistryError> {
    let mut scopes = BTreeMap::new();
    let pairs = url::form_urlencoded::parse(
        query.uri
            .query()
            .unwrap_or_default()
            .as_bytes(),
    );
    for (key, value) in pairs {
        if key == "service" && value != "pnpr" {
            return Err(RegistryError::BadRequest {
                reason: "invalid OCI token service".to_string(),
            });
        }
        if key != "scope" {
            continue;
        }
        for scope in value.split_whitespace() {
            grant_one_scope(
                state,
                identity,
                target,
                &mut GrantOne {
                    scope,
                    readonly: query.readonly,
                    scopes: &mut scopes,
                },
            )?;
        }
    }
    Ok(scopes)
}

fn grant_one_scope(
    state: &AppState,
    identity: &Identity,
    target: &str,
    grant: &mut GrantOne<'_>,
) -> Result<(), RegistryError> {
    let (resource, name, actions) = parse_scope(grant.scope)?;
    if resource != "repository" && resource != "repository(plugin)" {
        return Ok(());
    }
    let Ok(name) =
        pnpr_package_name::CanonicalPackageName::parse(name, pnpr_registry::Ecosystem::Oci)
    else {
        return Ok(());
    };
    if grant.scopes.len() >= 32 && !grant.scopes.contains_key(name.as_str()) {
        return Err(RegistryError::BadRequest {
            reason: "too many OCI token scopes".to_string(),
        });
    }
    let source =
        resolve_ecosystem_source(state, target, pnpr_registry::Ecosystem::Oci, name.as_str());
    let allowed: &mut Vec<String> = grant.scopes
        .entry(name.as_str().to_string())
        .or_default();
    extend_granted_actions(
        state,
        identity,
        &source,
        &mut GrantActions {
            name: name.as_str(),
            actions,
            readonly: grant.readonly,
            allowed,
        },
    );
    Ok(())
}

fn extend_granted_actions(
    state: &AppState,
    identity: &Identity,
    source: &crate::server::RegistrySource,
    grant: &mut GrantActions<'_>,
) {
    for action in grant.actions.split(',') {
        if grant.allowed
            .iter()
            .any(|held| held == action)
        {
            continue;
        }
        let granted = GrantAction {
            name: grant.name,
            action,
            readonly: grant.readonly,
        };
        if grant_action(state, identity, source, &granted) {
            grant.allowed.push(action.to_string());
        }
    }
}

/// Whether the caller may perform `action` on the repository. Reading it is
/// required for every action, so an unreadable repository grants nothing.
fn grant_action(
    state: &AppState,
    identity: &Identity,
    source: &crate::server::RegistrySource,
    grant: &GrantAction<'_>,
) -> bool {
    let operation = match grant.action {
        "pull" => Action::Access,
        "push" => Action::Publish,
        "delete" => Action::Unpublish,
        _ => return false,
    };
    if grant.action != "pull" && grant.readonly {
        return false;
    }
    authorize(state, identity, source, grant.name, Action::Access).is_ok()
        && authorize(state, identity, source, grant.name, operation).is_ok()
}

fn parse_scope(scope: &str) -> Result<(&str, &str, &str), RegistryError> {
    let invalid = || RegistryError::BadRequest {
        reason: "invalid OCI token scope".to_string(),
    };
    let Some((resource, remainder)) = scope.split_once(':') else {
        return Err(invalid());
    };
    let Some((name, actions)) = remainder.rsplit_once(':') else {
        return Err(invalid());
    };
    Ok((resource, name, actions))
}
