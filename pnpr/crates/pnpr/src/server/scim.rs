//! SCIM 2.0 user provisioning under `/-/pnpr/v0/scim/v2`, for identity
//! providers that deprovision accounts. Mounted only with `auth.scim`, and
//! authenticated only by its `token`.
//!
//! Resource IDs are immutable account IDs. Login names remain mutable attributes.
//! Deactivation revokes credentials; deletion also frees the login name.

pub(super) use directory::{ScimState, is_deprovisioned, reload_directory};

mod directory;
mod subjects;
pub(super) use subjects::{resolve_subject, validate_oidc};
mod discovery;
mod resources;
#[cfg(test)]
mod tests;

use super::{
    AppState, MAX_LOGIN_BODY_BYTES, RegistryError, Response, StatusCode,
    authentication::carries_bearer_secret, parse_json_body,
};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, FromRequestParts, Path, Query, State},
    http::{header, request::Parts},
    response::IntoResponse,
    routing::get,
};
use directory::{
    ScimUser, clean_up_before_reactivating, deprovision, insert_user, patch_user as patch_stored,
    remove_user, replace_user as replace_stored,
};
use resources::{list_response, parse_filter, parse_user, user_resource};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;

const BASE: &str = "/-/pnpr/v0/scim/v2";
const ERROR_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:Error";

pub(super) fn scim_routes() -> Router<AppState> {
    Router::new()
        .route(&format!("{BASE}/ServiceProviderConfig"), get(discovery::service_provider_config))
        .route(&format!("{BASE}/ResourceTypes"), get(discovery::resource_types))
        .route(&format!("{BASE}/Schemas"), get(discovery::schemas))
        .route(&format!("{BASE}/Users"), get(list_users).post(create_user))
        .route(
            &format!("{BASE}/Users/{{id}}"),
            get(get_user)
                .put(replace_user)
                .patch(patch_user)
                .delete(delete_user),
        )
        .route_layer(DefaultBodyLimit::max(MAX_LOGIN_BODY_BYTES))
}

/// A request carrying the `auth.scim.token`.
struct ScimClient;

impl FromRequestParts<AppState> for ScimClient {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Response> {
        let expected = state.inner.config.identity.auth.scim.as_ref().map(|scim| &scim.token);
        match expected {
            Some(expected) if carries_bearer_secret(&parts.headers, expected) => Ok(Self),
            _ => Err(scim_error(StatusCode::UNAUTHORIZED, "a valid SCIM bearer token is required")),
        }
    }
}

#[derive(Deserialize)]
struct UserPath {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListQuery {
    filter: Option<String>,
    start_index: Option<usize>,
    count: Option<usize>,
}

/// `GET /Users`, optionally with `filter=userName eq "<name>"`.
async fn list_users(
    State(state): State<AppState>,
    _client: ScimClient,
    Query(query): Query<ListQuery>,
) -> Response {
    let result = async {
        let wanted = query.filter
            .as_deref()
            .map(parse_filter)
            .transpose()?;
        let matching = matching_users(&state, wanted.as_deref()).await?;
        let start_index = query.start_index.unwrap_or(1).max(1);
        let page: Vec<_> = matching
            .iter()
            .skip(start_index - 1)
            .take(query.count.unwrap_or(usize::MAX))
            .map(|(id, user)| user_resource(id, user, &location(&state, id)))
            .collect();
        Ok(scim_json(StatusCode::OK, &list_response(&page, matching.len(), start_index)))
    };
    scim_respond(result.await)
}

/// `GET /Users/{id}`.
async fn get_user(
    State(state): State<AppState>,
    _client: ScimClient,
    Path(path): Path<UserPath>,
) -> Response {
    let result = async {
        let user = ensure_resource_exists(&state, &path.id).await?;
        Ok(scim_json(StatusCode::OK, &user_resource(&path.id, &user, &location(&state, &path.id))))
    };
    scim_respond(result.await)
}

/// `POST /Users`. A username a `DELETE` removed can be provisioned again.
async fn create_user(State(state): State<AppState>, _client: ScimClient, body: Bytes) -> Response {
    let result = async {
        let (username, user) = parse_user(&parse_json(&body)?)?;
        let id = state.inner.identity.auth.users.provision_user(&username).await?;
        if let Err(err) = insert_user(&state, &id, &user).await {
            state.inner.identity.auth.users.delete_user(&id).await?;
            return Err(err);
        }
        let response = finish_write(&state, &id, &user, StatusCode::CREATED).await?;
        tracing::info!(
            target: "pnpr::audit",
            action = "scim.user_create",
            actor = "scim",
            user = %username,
            active = user.active,
            "scim user created",
        );
        Ok(response)
    };
    scim_respond(result.await)
}

/// `PUT /Users/{id}`: replace the user's attributes and `active`.
async fn replace_user(
    State(state): State<AppState>,
    _client: ScimClient,
    Path(path): Path<UserPath>,
    body: Bytes,
) -> Response {
    let result = async {
        let (username, user) = parse_user(&parse_json(&body)?)?;
        clean_up_before_reactivating(&state, &path.id).await?;
        replace_stored(&state, &path.id, &user).await?;
        let response = finish_write(&state, &path.id, &user, StatusCode::OK).await?;
        tracing::info!(
            target: "pnpr::audit",
            action = "scim.user_replace",
            actor = "scim",
            user = %username,
            user_id = %path.id,
            active = user.active,
            "scim user replaced",
        );
        Ok(response)
    };
    scim_respond(result.await)
}

/// `PATCH /Users/{id}`.
async fn patch_user(
    State(state): State<AppState>,
    _client: ScimClient,
    Path(path): Path<UserPath>,
    body: Bytes,
) -> Response {
    let result = async {
        let patch = parse_json(&body)?;
        clean_up_before_reactivating(&state, &path.id).await?;
        let patched = patch_stored(&state, &path.id, &patch).await?;
        let response = finish_write(&state, &path.id, &patched, StatusCode::OK).await?;
        tracing::info!(
            target: "pnpr::audit",
            action = "scim.user_patch",
            actor = "scim",
            user = %patched.username,
            user_id = %path.id,
            active = patched.active,
            "scim user patched",
        );
        Ok(response)
    };
    scim_respond(result.await)
}

/// `DELETE /Users/{id}`: deprovision the user and stop listing it.
async fn delete_user(
    State(state): State<AppState>,
    _client: ScimClient,
    Path(path): Path<UserPath>,
) -> Response {
    let result = async {
        let username = state.inner.identity.auth.users
            .account_name(&path.id)
            .await?
            .ok_or(RegistryError::NotFound)?;
        remove_user(&state, &path.id).await?;
        deprovision(&state, &path.id).await?;
        state.inner.identity.auth.users.delete_user(&path.id).await?;
        tracing::info!(
            target: "pnpr::audit",
            action = "scim.user_delete",
            actor = "scim",
            user = %username,
            user_id = %path.id,
            "scim user deleted",
        );
        Ok(StatusCode::NO_CONTENT.into_response())
    };
    scim_respond(result.await)
}

/// Deprovision `user` when the write left it inactive, then answer with it.
async fn finish_write(
    state: &AppState,
    user_id: &str,
    user: &ScimUser,
    status: StatusCode,
) -> Result<Response, RegistryError> {
    let mut user = user.clone();
    user.username = state.inner.identity.auth.users
        .account_name(user_id)
        .await?
        .ok_or(RegistryError::NotFound)?;
    if !user.active {
        deprovision(state, user_id).await?;
    }
    Ok(scim_json(status, &user_resource(user_id, &user, &location(state, user_id))))
}

fn location(state: &AppState, user_id: &str) -> String {
    let public_url = state.inner.config.http.public_url.trim_end_matches('/');
    let users = format!("{public_url}{BASE}/Users");
    let Ok(mut url) = url::Url::parse(&users) else {
        return format!("{users}/{user_id}");
    };
    if let Ok(mut segments) = url.path_segments_mut() {
        segments.push(user_id);
    }
    url.into()
}

fn parse_json(body: &Bytes) -> Result<Value, RegistryError> {
    parse_json_body(body)
}

fn scim_json(status: StatusCode, body: &Value) -> Response {
    let mut response = (status, Body::from(body.to_string())).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, header::HeaderValue::from_static("application/scim+json"));
    response
}

fn scim_error(status: StatusCode, detail: &str) -> Response {
    let mut body = json!({ "schemas": [ERROR_SCHEMA], "status": status.as_u16().to_string(), "detail": detail });
    if status == StatusCode::CONFLICT {
        body["scimType"] = json!("uniqueness");
    }
    scim_json(status, &body)
}

fn scim_respond(result: Result<Response, RegistryError>) -> Response {
    result.unwrap_or_else(|err| {
        if err.status_code().is_server_error() {
            tracing::error!(error = %err, "SCIM request failed");
        }
        scim_error(err.status_code(), &err.public_message())
    })
}

async fn ensure_resource_exists(state: &AppState, id: &str) -> Result<ScimUser, RegistryError> {
    let stored = state.inner.storage.read_scim_user(id).await?.ok_or(RegistryError::NotFound)?;
    let username =
        state.inner.identity.auth.users.account_name(id).await?.ok_or(RegistryError::NotFound)?;
    Ok(ScimUser { username, active: stored.active, attributes: stored.attributes })
}

async fn matching_users(
    state: &AppState,
    wanted: Option<&str>,
) -> Result<Vec<(String, ScimUser)>, RegistryError> {
    if let Some(username) = wanted {
        return Ok(matching_filtered_user(state, username).await?.into_iter().collect());
    }
    let users = &state.inner.identity.auth.users;
    let stored_users = state.inner.storage.list_active_scim_users().await?;
    let user_ids: Vec<String> = stored_users
        .iter()
        .map(|user| user.user_id.clone())
        .collect();
    let accounts: HashMap<String, String> = users
        .accounts_by_id(&user_ids)
        .await?
        .into_iter()
        .map(|account| (account.id, account.username))
        .collect();
    Ok(stored_users
        .into_iter()
        .filter_map(|stored| {
            let username = accounts.get(&stored.user_id)?.clone();
            Some((
                stored.user_id,
                ScimUser { username, active: stored.active, attributes: stored.attributes },
            ))
        })
        .collect())
}

async fn matching_filtered_user(
    state: &AppState,
    username: &str,
) -> Result<Option<(String, ScimUser)>, RegistryError> {
    let users = &state.inner.identity.auth.users;
    let Some(id) = users.account_id(username).await? else {
        return Ok(None);
    };
    let Some(stored) = state.inner.storage.read_scim_user(&id).await? else {
        return Ok(None);
    };
    let Some(canonical_name) = users.account_name(&id).await? else {
        return Ok(None);
    };
    if canonical_name != username {
        return Ok(None);
    }
    let user =
        ScimUser { username: canonical_name, active: stored.active, attributes: stored.attributes };
    Ok(Some((id, user)))
}
