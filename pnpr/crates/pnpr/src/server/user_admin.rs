//! The admin API for accounts: `/-/pnpr/v0/admin/users`. Every endpoint
//! requires one of the `auth.admins`.

use super::{
    AppState, AuthedCaller, MAX_LOGIN_BODY_BYTES, RegistryError, Response, StatusCode, json,
    json_response, private_no_cache, require_admin, team_mutations::respond,
    tokens::token_response_object,
};
use crate::UserRemoval;
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    routing::{delete, get},
};
use pnpr_auth::UserPut;
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
        let (status, action) =
            match state.inner.identity.auth.put_user(&path.user, &password).await? {
                Some(UserPut::Created) => (StatusCode::CREATED, "user.create"),
                Some(UserPut::PasswordChanged) => (StatusCode::OK, "user.password_update"),
                None => return Err(RegistryError::NotFound),
            };
        let actor = identity.username().unwrap_or("-");
        tracing::info!(
            target: "pnpr::audit",
            action,
            actor = %actor,
            user = %path.user,
            "admin action performed on user",
        );
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
        let revoked_before = state.inner.identity.auth.revoke_all_tokens(&path.user).await?;
        let removed = state.inner.identity.auth.users.delete_user(&path.user).await?;
        state.inner.identity.oidc.revoke_user_sessions(&path.user);
        let revoked_after = state.inner.identity.auth.revoke_all_tokens(&path.user).await?;
        let had_nothing_to_revoke = revoked_before + revoked_after == 0;
        match removed {
            // A user the store never held, who also left no tokens behind,
            // was never there to delete.
            UserRemoval::NoSuchUser if had_nothing_to_revoke => {
                return Err(RegistryError::NotFound);
            }
            UserRemoval::NoSuchUser | UserRemoval::Removed => {}
        }
        let actor = identity.username().unwrap_or("-");
        let revoked_total = revoked_before + revoked_after;
        tracing::info!(
            target: "pnpr::audit",
            action = "user.delete",
            actor = %actor,
            user = %path.user,
            tokens_revoked = revoked_total,
            "admin user deleted",
        );
        Ok(StatusCode::NO_CONTENT)
    };
    respond(result.await)
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
                let actor = identity.username().unwrap_or("-");
                tracing::info!(
                    target: "pnpr::audit",
                    action = "user.token_revoke",
                    actor = %actor,
                    user = %path.user,
                    token_key = %path.key,
                    "admin user token revoked",
                );
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
    if password.len() > pnpr_auth::MAX_PASSWORD_BYTES {
        return Err(RegistryError::BadRequest {
            reason: format!("password cannot exceed {} bytes", pnpr_auth::MAX_PASSWORD_BYTES),
        });
    }
    Ok(password)
}
