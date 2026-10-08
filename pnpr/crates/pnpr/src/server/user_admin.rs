//! The admin API for accounts: `/-/pnpr/v0/admin/users`. Every endpoint
//! requires one of the `auth.admins`.

use super::{
    AppState, AuthedCaller, MAX_LOGIN_BODY_BYTES, RegistryError, Response, StatusCode, json,
    json_response, private_no_cache, require_admin, team_mutations::respond,
    user_accounts::token_response_object,
};
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    routing::{delete, get},
};
use serde::Deserialize;

const USERS: &str = "/-/pnpr/v0/admin/users";

pub(super) fn user_admin_routes() -> Router<AppState> {
    Router::new()
        .route(USERS, get(list_users))
        .route(
            &format!("{USERS}/{{user}}"),
            delete(delete_user)
                .put(put_user)
                .route_layer(DefaultBodyLimit::max(MAX_LOGIN_BODY_BYTES)),
        )
        .route(&format!("{USERS}/{{user}}/tokens"), get(list_user_tokens))
        .route(&format!("{USERS}/{{user}}/tokens/{{key}}"), delete(revoke_user_token))
}

#[derive(Deserialize)]
struct UserPath {
    user: String,
}

#[derive(Deserialize)]
struct UserTokenPath {
    user: String,
    key: String,
}

/// The body of `PUT /-/pnpr/v0/admin/users/{user}`.
#[derive(Deserialize)]
struct NewPassword {
    password: String,
}

/// `GET /-/pnpr/v0/admin/users` — `{"users": [{"name": ...}]}`, sorted.
async fn list_users(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
) -> Response {
    let result = async {
        require_admin(&state, &identity, "list", "the users".to_string())?;
        let names = state.inner.identity.auth.users.list_users().await?;
        let users: Vec<_> = names
            .into_iter()
            .map(|name| json!({ "name": name }))
            .collect();
        Ok(json_response(StatusCode::OK, &json!({ "users": users })))
    };
    private_no_cache(respond(result.await))
}

/// `PUT /-/pnpr/v0/admin/users/{user}` with `{"password": ...}` — create the
/// user (201) or replace its password (200). Creation ignores the
/// self-registration cap.
async fn put_user(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(path): Path<UserPath>,
    body: Bytes,
) -> Response {
    let result = async {
        require_admin(&state, &identity, "manage", format!("user {:?}", path.user))?;
        let password = parse_password(&body)?;
        let users = &state.inner.identity.auth.users;
        let status = if users.create_user(&path.user, &password).await? {
            StatusCode::CREATED
        } else if users.set_password(&path.user, &password).await? {
            StatusCode::OK
        } else {
            // Another request removed the user between the two calls.
            return Err(RegistryError::NotFound);
        };
        Ok(json_response(status, &json!({ "name": path.user })))
    };
    respond(result.await)
}

/// `DELETE /-/pnpr/v0/admin/users/{user}` — remove the user and revoke every
/// token it holds. The tokens are revoked before the account goes, so a
/// failed revocation leaves the account in place, and again after, for any
/// token a login racing the removal issued. A retried request finishes a
/// removal whose second sweep failed.
async fn delete_user(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(path): Path<UserPath>,
) -> Response {
    let result = async {
        require_admin(&state, &identity, "remove", format!("user {:?}", path.user))?;
        let revoked_before = revoke_all_tokens(&state, &path.user).await?;
        let removed = state.inner.identity.auth.users.delete_user(&path.user).await?;
        let revoked_after = revoke_all_tokens(&state, &path.user).await?;
        if !removed && revoked_before + revoked_after == 0 {
            return Err(RegistryError::NotFound);
        }
        Ok(StatusCode::NO_CONTENT)
    };
    respond(result.await)
}

/// Revoke every token `user` holds, returning how many there were.
async fn revoke_all_tokens(state: &AppState, user: &str) -> Result<usize, RegistryError> {
    let tokens = &state.inner.identity.auth.tokens;
    let held = tokens.list_for_user(user).await?;
    for (key, _) in &held {
        tokens.revoke_by_key(key).await?;
    }
    Ok(held.len())
}

/// `GET /-/pnpr/v0/admin/users/{user}/tokens` — the user's tokens in the
/// `npm token list` shape.
async fn list_user_tokens(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(path): Path<UserPath>,
) -> Response {
    let result = async {
        require_admin(&state, &identity, "list", format!("the tokens of {:?}", path.user))?;
        let tokens = state.inner.identity.auth.tokens.list_for_user(&path.user).await?;
        let objects: Vec<_> = tokens
            .iter()
            .map(|(key, record)| token_response_object(key, record))
            .collect();
        Ok(json_response(StatusCode::OK, &json!({ "objects": objects })))
    };
    private_no_cache(respond(result.await))
}

/// `DELETE /-/pnpr/v0/admin/users/{user}/tokens/{key}` — revoke one of the
/// user's tokens. A key the user does not hold is not found.
async fn revoke_user_token(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(path): Path<UserTokenPath>,
) -> Response {
    let result = async {
        require_admin(&state, &identity, "revoke", format!("the tokens of {:?}", path.user))?;
        let tokens = &state.inner.identity.auth.tokens;
        match tokens.find_by_key(&path.key).await? {
            Some(record) if record.username == path.user => {
                tokens.revoke_by_key(&path.key).await?;
                Ok(StatusCode::NO_CONTENT)
            }
            _ => Err(RegistryError::NotFound),
        }
    };
    respond(result.await)
}

fn parse_password(body: &Bytes) -> Result<String, RegistryError> {
    let NewPassword { password } = serde_json::from_slice(body)
        .map_err(|err| RegistryError::BadRequest {
            reason: format!("invalid request body: {err}"),
        })?;
    if password.is_empty() {
        return Err(RegistryError::BadRequest { reason: "password must not be empty".to_string() });
    }
    Ok(password)
}
