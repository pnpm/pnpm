//! SCIM 2.0 user provisioning under `/-/pnpr/v0/scim/v2`, for identity
//! providers that deprovision accounts. Mounted only with `auth.scim`, and
//! authenticated only by its `token`.
//!
//! A user's SCIM `id` is its pnpr username. Making a user inactive or
//! deleting it revokes its tokens, password account, and browser sessions,
//! and every replica then refuses the username (see
//! [`directory::is_deprovisioned`]).

pub(super) use directory::{ScimState, is_deprovisioned, reload_directory};

mod directory;
mod discovery;
mod resources;

use super::{
    AppState, MAX_LOGIN_BODY_BYTES, RegistryError, Response, StatusCode, ecosystem::sha256_hex,
};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, FromRequestParts, Path, Query, State},
    http::{header, request::Parts},
    response::IntoResponse,
    routing::get,
};
use directory::{deprovision, read_directory, update_directory};
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
        let sent = parts.headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "));
        // Comparing digests keeps the comparison time independent of how
        // much of the secret a guess gets right.
        match (expected, sent) {
            (Some(expected), Some(sent))
                if sha256_hex(expected.as_bytes()) == sha256_hex(sent.trim().as_bytes()) =>
            {
                Ok(Self)
            }
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
        let matching: Vec<_> = directory.users
            .iter()
            .filter(|(name, user)| {
                !user.removed
                    && wanted
                        .as_ref()
                        .is_none_or(|wanted| wanted == *name)
            })
            .collect();
        let start_index = query.start_index.unwrap_or(1).max(1);
        let page: Vec<_> = matching
            .iter()
            .skip(start_index - 1)
            .take(query.count.unwrap_or(usize::MAX))
            .map(|(name, user)| user_resource(name, user, &location(&state, name)))
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
        let user = directory.users
            .get(&path.id)
            .filter(|user| !user.removed)
            .ok_or(RegistryError::NotFound)?;
        Ok(scim_json(StatusCode::OK, &user_resource(&path.id, user, &location(&state, &path.id))))
    };
    scim_respond(result.await)
}

/// `POST /Users`. A username a `DELETE` removed can be provisioned again.
async fn create_user(State(state): State<AppState>, _client: ScimClient, body: Bytes) -> Response {
    let result = async {
        let (username, user) = parse_user(&parse_json(&body)?)?;
        let created = update_directory(&state, |directory| {
            if directory.users.get(&username).is_some_and(|existing| !existing.removed) {
                return Err(RegistryError::AdminConflict {
                    reason: format!("user {username:?} already exists"),
                });
            }
            directory.users.insert(username.clone(), user.clone());
            Ok(user.clone())
        })
        .await?;
        finish_write(&state, &username, &created, StatusCode::CREATED).await
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
        if username != path.id {
            return Err(RegistryError::BadRequest { reason: "userName cannot change".to_string() });
        }
        let replaced = update_directory(&state, |directory| {
            let existing = existing_user(directory, &path.id)?;
            *existing = user.clone();
            Ok(existing.clone())
        })
        .await?;
        finish_write(&state, &path.id, &replaced, StatusCode::OK).await
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
        let patched = update_directory(&state, |directory| {
            let existing = existing_user(directory, &path.id)?;
            let mut user = existing.clone();
            apply_patch(&mut user, &patch)?;
            *existing = user;
            Ok(existing.clone())
        })
        .await?;
        finish_write(&state, &path.id, &patched, StatusCode::OK).await
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
        update_directory(&state, |directory| {
            let existing = existing_user(directory, &path.id)?;
            existing.active = false;
            existing.removed = true;
            Ok(())
        })
        .await?;
        deprovision(&state, &path.id).await?;
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
    if !user.active {
        deprovision(state, username).await?;
    }
    Ok(scim_json(status, &user_resource(username, user, &location(state, username))))
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
