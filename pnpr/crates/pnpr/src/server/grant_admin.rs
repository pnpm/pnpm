//! The grant admin API: `/-/pnpr/v0/admin/grants`.
//!
//! One of the `auth.admins` or `auth.registryAdmins` reads and writes every
//! grant. A caller a grant gives `manage` may replace that grant's subjects,
//! as long as the change leaves who has `manage` as it was.

use super::{
    AppState, AuthedCaller, Identity, RegistryError, Response, StatusCode,
    grant_directory::{Grants, reload_written_grants, validate, written},
    json_response, parse_json_body, private_no_cache,
    registry_graph::{if_match, versioned},
    request_access::require_access_admin,
    require_caller,
    team_mutations::respond,
};
use crate::audit::Actor;
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::HeaderMap,
    response::IntoResponse,
    routing::get,
};
use indexmap::IndexMap;
use pnpr_policy::{AccessToken, Action};
use pnpr_storage::database::{GrantContent, StoredGrant};
use serde::Deserialize;
use serde_json::{Value, json};

/// The largest grant the API accepts.
const MAX_GRANT_BODY_BYTES: usize = 1024 * 1024;

pub(super) fn grant_admin_routes() -> Router<AppState> {
    Router::new()
        .route("/-/pnpr/v0/admin/grants", get(list_grants))
        .route(
            "/-/pnpr/v0/admin/grants/{name}",
            get(get_grant)
                .put(put_grant)
                .delete(delete_grant)
                .route_layer(DefaultBodyLimit::max(MAX_GRANT_BODY_BYTES)),
        )
}

/// The body of a `PUT`: a grant without its name, which the path gives.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantBody {
    registries: Vec<String>,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    /// Each subject with the actions it is given.
    #[serde(default)]
    subjects: IndexMap<String, Vec<String>>,
}

impl GrantBody {
    fn into_content(self, name: &str) -> GrantContent {
        let mut subjects: Vec<(String, String)> = self.subjects
            .into_iter()
            .flat_map(|(subject, actions)| {
                actions
                    .into_iter()
                    .map(move |action| (subject.clone(), action))
            })
            .collect();
        subjects.sort();
        subjects.dedup();
        let sorted = |mut list: Vec<String>| {
            list.sort();
            list.dedup();
            list
        };
        GrantContent {
            name: name.to_string(),
            registries: sorted(self.registries),
            include: sorted(self.include),
            exclude: sorted(self.exclude),
            subjects,
        }
    }
}

/// A grant in the shape the API reads and writes.
fn grant_json(content: &GrantContent) -> Value {
    let mut subjects: IndexMap<&str, Vec<&str>> = IndexMap::new();
    for (subject, action) in &content.subjects {
        subjects
            .entry(subject)
            .or_default()
            .push(action);
    }
    json!({
        "name": content.name,
        "registries": content.registries,
        "include": content.include,
        "exclude": content.exclude,
        "subjects": subjects,
    })
}

fn version_of(grants: &Grants) -> String {
    grants.generation.unwrap_or(0).to_string()
}

/// This replica's copy of the grants, read again from the database first, so
/// an admin sees what another replica just wrote.
async fn current(state: &AppState) -> Result<std::sync::Arc<Grants>, RegistryError> {
    reload_written_grants(state).await?;
    Ok(state.inner.identity.managed.grants.snapshot())
}

/// `GET /-/pnpr/v0/admin/grants`: every grant, with the version of the
/// grants as the `ETag`.
async fn list_grants(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
) -> Response {
    let result = async {
        require_access_admin(&state, &identity, "read", "the grants".to_string())?;
        let grants = current(&state).await?;
        let listed: Vec<Value> = grants.grants
            .iter()
            .map(|grant| grant_json(&grant.content))
            .collect();
        let version = version_of(&grants);
        let body = json!({ "version": version, "grants": listed });
        Ok(versioned(json_response(StatusCode::OK, &body), &version))
    };
    private_no_cache(respond(result.await))
}

/// `GET /-/pnpr/v0/admin/grants/{name}`.
async fn get_grant(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(name): Path<String>,
) -> Response {
    let result = async {
        require_caller(&identity, "the grants")?;
        let grants = current(&state).await?;
        let grant = grants.named(&name).ok_or(RegistryError::NotFound)?;
        if !manages(&identity, &grant.content) {
            // A caller who may not see the grant learns nothing of it.
            require_access_admin(&state, &identity, "read", format!("grant {name:?}"))
                .map_err(|_| RegistryError::NotFound)?;
        }
        let version = version_of(&grants);
        Ok(versioned(json_response(StatusCode::OK, &grant_json(&grant.content)), &version))
    };
    private_no_cache(respond(result.await))
}

/// `PUT /-/pnpr/v0/admin/grants/{name}`: create the grant or replace what it
/// holds. `If-Match` names the version of the grants the change is based on.
async fn put_grant(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let result = async {
        require_caller(&identity, "the grants")?;
        let content = parse_json_body::<GrantBody>(&body)?.into_content(&name);
        let grants = current(&state).await?;
        let expected = expected_generation(&headers, &grants)?;
        let existing = grants.named(&name);
        let is_admin =
            require_access_admin(&state, &identity, "change", format!("grant {name:?}")).is_ok();
        if !is_admin {
            let existing = existing
                .filter(|grant| manages(&identity, &grant.content))
                .ok_or(RegistryError::NotFound)?;
            check_managed_change(&identity, &existing.content, &content)?;
        }
        validate(&state, &content)?;
        let id =
            existing.map_or_else(pnpr_auth::new_principal_id, |grant| grant.id.clone());
        let stored = state.inner.storage.put_grant(&id, content.clone(), expected).await?;
        let version = written(stored, expected)?.to_string();
        reload_written_grants(&state).await?;
        let actor = Actor::of(&identity);
        crate::audit!(actor, "grant.put", grant = %name, version = %version, "grant stored");
        Ok(versioned(json_response(StatusCode::OK, &grant_json(&content)), &version))
    };
    private_no_cache(respond(result.await))
}

/// `DELETE /-/pnpr/v0/admin/grants/{name}`.
async fn delete_grant(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Response {
    let result = async {
        require_access_admin(&state, &identity, "remove", format!("grant {name:?}"))?;
        let grants = current(&state).await?;
        let expected = expected_generation(&headers, &grants)?;
        let StoredGrant { id, .. } = grants.named(&name).ok_or(RegistryError::NotFound)?;
        let version =
            written(state.inner.storage.remove_grant(id, expected).await?, expected)?.to_string();
        reload_written_grants(&state).await?;
        let actor = Actor::of(&identity);
        crate::audit!(actor, "grant.delete", grant = %name, version = %version, "grant removed");
        Ok(versioned(StatusCode::NO_CONTENT.into_response(), &version))
    };
    private_no_cache(respond(result.await))
}

/// The generation an `If-Match` header asks the grants to be at. A header
/// that names other versions than the current one is refused here, before
/// the write.
fn expected_generation(headers: &HeaderMap, grants: &Grants) -> Result<Option<u64>, RegistryError> {
    let Some(versions) = if_match(headers)? else {
        return Ok(None);
    };
    let current = version_of(grants);
    if versions.contains(&current) {
        return Ok(grants.generation);
    }
    Err(RegistryError::PreconditionFailed {
        resource: "the grants".to_string(),
        expected: versions.join(", "),
    })
}

/// Whether `content` gives `identity` `manage`.
fn manages(identity: &Identity, content: &GrantContent) -> bool {
    content.subjects
        .iter()
        .filter(|(_, action)| action == Action::Manage.as_str())
        .any(|(subject, _)| {
            pnpr_policy::AccessList::new(vec![AccessToken::parse(subject)]).allows(identity)
        })
}

/// The subjects `content` gives `manage`, sorted.
fn managers(content: &GrantContent) -> Vec<&str> {
    let mut managers: Vec<&str> = content.subjects
        .iter()
        .filter(|(_, action)| action == Action::Manage.as_str())
        .map(|(subject, _)| subject.as_str())
        .collect();
    managers.sort_unstable();
    managers
}

/// A caller who manages a grant may change its subjects, and nothing else:
/// not the registries or patterns it covers, and not who has `manage`.
fn check_managed_change(
    identity: &Identity,
    existing: &GrantContent,
    next: &GrantContent,
) -> Result<(), RegistryError> {
    let same_scope = existing.registries == next.registries
        && existing.include == next.include
        && existing.exclude == next.exclude;
    if same_scope && managers(existing) == managers(next) {
        return Ok(());
    }
    Err(RegistryError::Forbidden {
        user: identity
            .username()
            .unwrap_or_default()
            .to_string(),
        action: "change",
        resource: format!(
            "more than the subjects of grant {:?}; `manage` covers the subjects, less `manage` itself",
            existing.name,
        ),
    })
}
