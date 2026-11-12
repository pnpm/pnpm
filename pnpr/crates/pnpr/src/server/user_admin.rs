//! The admin API for accounts: `/-/pnpr/v0/admin/users`. Every endpoint
//! requires one of the `auth.admins`.

use super::{
    AppState, AuthedCaller, MAX_LOGIN_BODY_BYTES, RegistryError, Response, StatusCode, json,
    json_response, parse_json_body, private_no_cache, require_admin, team_mutations::respond,
    tokens::token_response_object,
};
use crate::{PasswordChange, UserCreation, UserRemoval};
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
        .route(USERS, get(list_users).post(create_user))
        .route(
            &format!("{USERS}/{{id}}"),
            delete(delete_user).put(set_user_password).patch(rename_user),
        )
        .route(&format!("{USERS}/{{id}}/tokens"), get(list_user_tokens))
        .route(&format!("{USERS}/{{id}}/tokens/{{key}}"), delete(revoke_user_token))
        .route_layer(DefaultBodyLimit::max(MAX_LOGIN_BODY_BYTES))
}

#[derive(Deserialize)]
struct UserPath {
    id: String,
}

#[derive(Deserialize)]
struct UserTokenPath {
    id: String,
    key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateUser {
    name: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewPassword {
    password: String,
}

/// `GET /-/pnpr/v0/admin/users`: `{"users": [{"id": ..., "name": ...}]}`, sorted.
async fn list_users(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
) -> Response {
    let result = async {
        require_admin(&state, &identity, "list", "the users".to_string())?;
        let accounts = state.inner.identity.auth.users.list_accounts().await?;
        let users: Vec<_> = accounts
            .into_iter()
            .map(|account| json!({ "id": account.id, "name": account.username }))
            .collect();
        Ok(json_response(StatusCode::OK, &json!({ "users": users })))
    };
    private_no_cache(respond(result.await))
}

/// `POST /-/pnpr/v0/admin/users` with `{"name": ..., "password": ...}` creates
/// an account (201), ignoring the self-registration cap.
async fn create_user(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    body: Bytes,
) -> Response {
    let result = async {
        require_admin(&state, &identity, "create", "an account".to_string())?;
        let CreateUser { name, password } = parse_json_body(&body)?;
        validate_password(&password)?;
        let UserCreation::Created(id) =
            state.inner.identity.auth.users.create_user(&name, &password).await?
        else {
            return Err(RegistryError::AdminConflict {
                reason: format!("username {name:?} is taken"),
            });
        };
        tracing::info!(
            target: "pnpr::audit",
            action = "user.create",
            actor = %identity.username().unwrap_or("-"),
            user = %name,
            user_id = %id,
            "admin user created",
        );
        Ok(json_response(StatusCode::CREATED, &json!({ "id": id, "name": name })))
    };
    respond(result.await)
}

/// `PUT /-/pnpr/v0/admin/users/{id}` with `{"password": ...}` replaces an
/// account's password (200) and revokes its tokens and browser sessions.
async fn set_user_password(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(path): Path<UserPath>,
    body: Bytes,
) -> Response {
    let result = async {
        require_admin(&state, &identity, "manage", format!("account {:?}", path.id))?;
        let NewPassword { password } = parse_json_body(&body)?;
        validate_password(&password)?;
        let auth = &state.inner.identity.auth;
        let Some(name) = auth.users.account_name(&path.id).await? else {
            return Err(RegistryError::NotFound);
        };
        let changed = auth.change_password(&path.id, &password).await;
        if let Err(error) = &changed {
            state.inner.identity.oidc.revoke_committed_password_sessions(error);
        }
        if changed? == PasswordChange::NoSuchUser {
            return Err(RegistryError::NotFound);
        }
        state.inner.identity.oidc.revoke_user_sessions(&path.id);
        tracing::info!(
            target: "pnpr::audit",
            action = "user.password_update",
            actor = %identity.username().unwrap_or("-"),
            user = %name,
            user_id = %path.id,
            "admin user password updated",
        );
        Ok(json_response(StatusCode::OK, &json!({ "id": path.id, "name": name })))
    };
    respond(result.await)
}

async fn delete_user(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(path): Path<UserPath>,
) -> Response {
    let result = async {
        require_admin(&state, &identity, "remove", format!("account {:?}", path.id))?;
        let auth = &state.inner.identity.auth;
        let name = auth.users
            .account_name(&path.id)
            .await?
            .unwrap_or_else(|| format!("deleted:{}", path.id));
        let removed = auth.users.delete_user(&path.id).await?;
        state.inner.identity.oidc.revoke_user_sessions(&path.id);
        let revoked_total = auth.revoke_all_tokens(&path.id).await?;
        if removed == UserRemoval::NoSuchUser && revoked_total == 0 {
            return Err(RegistryError::NotFound);
        }
        let actor = identity.username().unwrap_or("-");
        tracing::info!(
            target: "pnpr::audit",
            action = "user.delete",
            actor = %actor,
            user = %name,
            user_id = %path.id,
            tokens_revoked = revoked_total,
            "admin user deleted",
        );
        Ok(StatusCode::NO_CONTENT)
    };
    respond(result.await)
}

/// `GET /-/pnpr/v0/admin/users/{id}/tokens`: the account's tokens in the
/// `npm token list` shape.
async fn list_user_tokens(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(path): Path<UserPath>,
) -> Response {
    let result = async {
        require_admin(&state, &identity, "list", format!("the tokens of {:?}", path.id))?;
        let tokens = state.inner.identity.auth.tokens.list_for_user(&path.id).await?;
        let objects: Vec<_> = tokens
            .iter()
            .map(|(key, record)| token_response_object(key, record))
            .collect();
        Ok(json_response(StatusCode::OK, &json!({ "objects": objects })))
    };
    private_no_cache(respond(result.await))
}

/// `DELETE /-/pnpr/v0/admin/users/{id}/tokens/{key}` revokes one of the
/// account's tokens. A key the account does not hold is not found.
async fn revoke_user_token(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(path): Path<UserTokenPath>,
) -> Response {
    let result = async {
        require_admin(&state, &identity, "revoke", format!("the tokens of {:?}", path.id))?;
        let tokens = &state.inner.identity.auth.tokens;
        match tokens.find_by_key(&path.key).await? {
            Some(record) if record.user_id == path.id => {
                tokens.revoke_by_key(&path.key).await?;
                let actor = identity.username().unwrap_or("-");
                let name = state.inner.identity.auth.users
                    .account_name(&path.id)
                    .await?
                    .unwrap_or_else(|| format!("deleted:{}", path.id));
                tracing::info!(
                    target: "pnpr::audit",
                    action = "user.token_revoke",
                    actor = %actor,
                    user = %name,
                    user_id = %path.id,
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

fn validate_password(password: &str) -> Result<(), RegistryError> {
    if password.is_empty() {
        return Err(RegistryError::BadRequest { reason: "password must not be empty".to_string() });
    }
    if password.len() > pnpr_auth::MAX_PASSWORD_BYTES {
        return Err(RegistryError::BadRequest {
            reason: format!("password cannot exceed {} bytes", pnpr_auth::MAX_PASSWORD_BYTES),
        });
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameUser {
    name: String,
}

async fn rename_user(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(path): Path<UserPath>,
    body: Bytes,
) -> Response {
    let result = async {
        require_admin(&state, &identity, "rename", format!("account {:?}", path.id))?;
        let RenameUser { name } = parse_json_body(&body)?;
        state.inner.identity.auth.users.rename_user(&path.id, &name).await?;
        tracing::info!(
            target: "pnpr::audit",
            action = "user.rename",
            actor = %identity.username().unwrap_or("-"),
            user = %name,
            user_id = %path.id,
            "admin user renamed",
        );
        Ok(json_response(StatusCode::OK, &json!({ "id": path.id, "name": name })))
    };
    respond(result.await)
}
