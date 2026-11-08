//! SCIM 2.0 user provisioning under `/-/pnpr/v0/scim/v2`, for identity
//! providers that deprovision accounts. Mounted only with `auth.scim`, and
//! authenticated only by its `token`.
//!
//! Resource IDs are immutable account IDs. Login names remain mutable attributes.
//! Deactivation revokes credentials; deletion also frees the login name.

pub(super) use directory::{ScimState, is_deprovisioned, reload_directory};

mod directory;
mod discovery;
mod resources;
#[cfg(test)]
mod tests;

use super::{
    AppState, MAX_LOGIN_BODY_BYTES, RegistryError, Response, StatusCode,
    authentication::carries_bearer_secret,
};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, FromRequestParts, Path, Query, State},
    http::{header, request::Parts},
    response::IntoResponse,
    routing::get,
};
use directory::{clean_up_before_reactivating, deprovision, read_directory, update_directory};
use resources::{apply_patch, list_response, parse_filter, parse_user, user_resource};
use serde::Deserialize;
use serde_json::{Value, json};

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
        let (directory, _) = read_directory(&state).await?;
        let accounts = account_names(&state, &directory, wanted.as_deref()).await?;
        let mut matching = Vec::new();
        for (id, user) in &directory.users {
            if user.removed {
                continue;
            }
            let Some(username) = accounts.get(id) else {
                continue;
            };
            if wanted
                .as_ref()
                .is_some_and(|wanted| wanted != username)
            {
                continue;
            }
            let mut user = user.clone();
            user.username.clone_from(username);
            matching.push((id.clone(), user));
        }
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
        let (directory, _) = read_directory(&state).await?;
        let mut user = directory.users
            .get(&path.id)
            .filter(|user| !user.removed)
            .ok_or(RegistryError::NotFound)?
            .clone();
        user.username = state.inner.identity.auth.users
            .account_name(&path.id)
            .await?
            .ok_or(RegistryError::NotFound)?;
        Ok(scim_json(StatusCode::OK, &user_resource(&path.id, &user, &location(&state, &path.id))))
    };
    scim_respond(result.await)
}

/// `POST /Users`. A username a `DELETE` removed can be provisioned again.
async fn create_user(State(state): State<AppState>, _client: ScimClient, body: Bytes) -> Response {
    let result = async {
        let (username, user) = parse_user(&parse_json(&body)?)?;
        let id = state.inner.identity.auth.users.provision_user(&username).await?;
        let created =
            update_directory(&state, |directory| Ok(directory.store(&id, user.clone()))).await;
        let created = match created {
            Ok(created) => created,
            Err(err) => {
                state.inner.identity.auth.users.delete_user(&id).await?;
                return Err(err);
            }
        };
        let response = finish_write(&state, &id, &created, StatusCode::CREATED).await?;
        tracing::info!(
            target: "pnpr::audit",
            action = "scim.user_create",
            actor = "scim",
            user = %username,
            active = created.active,
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
        ensure_resource_exists(&state, &path.id).await?;
        state.inner.identity.auth.users.rename_user(&path.id, &username).await?;
        let replaced = update_directory(&state, |directory| {
            existing_user(directory, &path.id)?;
            Ok(directory.store(&path.id, user.clone()))
        })
        .await?;
        let response = finish_write(&state, &path.id, &replaced, StatusCode::OK).await?;
        tracing::info!(
            target: "pnpr::audit",
            action = "scim.user_replace",
            actor = "scim",
            user = %replaced.username,
            user_id = %path.id,
            active = replaced.active,
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
        let mut projected = ensure_resource_exists(&state, &path.id).await?;
        apply_patch(&mut projected, &patch)?;
        state.inner.identity.auth.users.rename_user(&path.id, &projected.username).await?;
        let patched = update_directory(&state, |directory| {
            let mut user = existing_user(directory, &path.id)?.clone();
            user.username.clone_from(&projected.username);
            apply_patch(&mut user, &patch)?;
            Ok(directory.store(&path.id, user))
        })
        .await?;
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
        update_directory(&state, |directory| {
            let mut user = directory.users
                .get(&path.id)
                .cloned()
                .ok_or(RegistryError::NotFound)?;
            user.active = false;
            user.removed = true;
            directory.store(&path.id, user);
            Ok(())
        })
        .await?;
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

fn existing_user<'a>(
    directory: &'a mut directory::Directory,
    username: &str,
) -> Result<&'a mut directory::ScimUser, RegistryError> {
    directory.users
        .get_mut(username)
        .filter(|user| !user.removed)
        .ok_or(RegistryError::NotFound)
}

/// Deprovision `user` when the write left it inactive, then answer with it.
async fn finish_write(
    state: &AppState,
    username: &str,
    user: &directory::ScimUser,
    status: StatusCode,
) -> Result<Response, RegistryError> {
    let mut user = user.clone();
    user.username = state.inner.identity.auth.users
        .account_name(username)
        .await?
        .ok_or(RegistryError::NotFound)?;
    if !user.active {
        deprovision(state, username).await?;
    }
    Ok(scim_json(status, &user_resource(username, &user, &location(state, username))))
}

fn location(state: &AppState, username: &str) -> String {
    let public_url = state.inner.config.http.public_url.trim_end_matches('/');
    let users = format!("{public_url}{BASE}/Users");
    let Ok(mut url) = url::Url::parse(&users) else {
        return format!("{users}/{username}");
    };
    if let Ok(mut segments) = url.path_segments_mut() {
        segments.push(username);
    }
    url.into()
}

fn parse_json(body: &Bytes) -> Result<Value, RegistryError> {
    serde_json::from_slice(body)
        .map_err(|err| RegistryError::BadRequest { reason: format!("invalid request body: {err}") })
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

async fn ensure_resource_exists(
    state: &AppState,
    id: &str,
) -> Result<directory::ScimUser, RegistryError> {
    let (directory, _) = read_directory(state).await?;
    let mut user = directory.users
        .get(id)
        .filter(|user| !user.removed)
        .ok_or(RegistryError::NotFound)?
        .clone();
    user.username =
        state.inner.identity.auth.users.account_name(id).await?.ok_or(RegistryError::NotFound)?;
    Ok(user)
}

async fn account_names(
    state: &AppState,
    directory: &directory::Directory,
    wanted: Option<&str>,
) -> Result<std::collections::HashMap<String, String>, RegistryError> {
    let users = &state.inner.identity.auth.users;
    let user_ids = if let Some(username) = wanted {
        users
            .account_id(username)
            .await?
            .into_iter()
            .collect()
    } else {
        directory.users
            .iter()
            .filter(|(_, user)| !user.removed)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>()
    };
    let user_ids: Vec<_> = user_ids
        .into_iter()
        .filter(|id| directory.users.get(id).is_some_and(|user| !user.removed))
        .collect();
    let accounts = users
        .accounts_by_id(&user_ids)
        .await?
        .into_iter()
        .map(|account| (account.id, account.username))
        .collect();
    Ok(accounts)
}
