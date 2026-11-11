use super::super::{
    AppState, AuthedCaller, Identity, RegistryError, Response, State, TargetRegistry,
    json_response, parse_json_body, private_no_cache, require_caller,
    tokens::verify_caller_password,
};
use axum::{body::Bytes, extract::Extension, http::StatusCode, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct ProfileUpdate {
    password: PasswordUpdate,
}

#[derive(Deserialize)]
struct PasswordUpdate {
    old: String,
    new: String,
}

pub(crate) async fn post_profile(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
    client_ip: Option<Extension<super::super::ClientIp>>,
    body: Bytes,
) -> Response {
    let client_ip = client_ip.map(|Extension(super::super::ClientIp(ip))| ip);
    if let Some(username) = identity.username()
        && let Err(retry_after) = state.inner.identity.login_rate_limiter.check(client_ip, username)
    {
        return private_no_cache(super::login_throttled_response(&state, retry_after));
    }
    let _permit = state.inner.identity.login_concurrency.acquire().await;
    if let Some(username) = identity.username()
        && let Err(retry_after) = state.inner.identity.login_rate_limiter.check(client_ip, username)
    {
        return private_no_cache(super::login_throttled_response(&state, retry_after));
    }
    private_no_cache(
        update_password(&state, &identity, client_ip, &body).await
            .unwrap_or_else(IntoResponse::into_response),
    )
}

async fn update_password(
    state: &AppState,
    identity: &Identity,
    client_ip: Option<std::net::IpAddr>,
    body: &[u8],
) -> Result<Response, RegistryError> {
    let username = require_caller(identity, "password change")?;
    let update: ProfileUpdate = serde_json::from_value(parse_json_body(body)?)
        .map_err(|error| RegistryError::BadRequest { reason: error.to_string() })?;
    let verified_hash =
        verify_caller_password(state, client_ip, &username, Some(&update.password.old)).await?;
    if verified_hash == pnpr_auth::NO_PASSWORD_HASH {
        return Err(RegistryError::Unauthenticated { resource: "a password account".to_string() });
    }
    let user_id = identity.user_id().expect("authenticated caller");
    if !state.inner.identity.auth.change_password_if_current(
        user_id,
        &update.password.new,
        &verified_hash,
    )
    .await?
    {
        return Err(RegistryError::Unauthenticated {
            resource: "the current password".to_string(),
        });
    }
    state.inner.identity.oidc.revoke_user_sessions(user_id);
    tracing::info!(target: "pnpr::audit", action = "user.password_update", actor = %username, user = %username, "account password changed");
    Ok(json_response(StatusCode::OK, &json!({ "name": username })))
}
