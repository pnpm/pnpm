mod payload;

use std::net::IpAddr;

use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use pnpr_auth::TokenRecord;
use pnpr_error::RegistryError;
use pnpr_policy::Identity;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    AppState, AuthedCaller, StatusCode, TargetRegistry, iso_from_unix_millis, json_response,
    not_found, private_no_cache, require_caller,
};

#[derive(Deserialize)]
pub(super) struct TokenPath {
    pub(super) token: String,
}

#[derive(Deserialize)]
pub(super) struct TokenKeyPath {
    pub(super) key: String,
}

pub(crate) async fn post_create_token(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
    client_ip: Option<axum::extract::Extension<super::ClientIp>>,
    body: axum::body::Bytes,
) -> Response {
    let ip = client_ip.map(|axum::extract::Extension(super::ClientIp(ip))| ip);
    private_no_cache(create_token(&state, ip, &identity, &body).await)
}

pub(crate) async fn get_token_list(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
) -> Response {
    private_no_cache(list_tokens(&state, &identity).await)
}

pub(crate) async fn delete_token_by_key(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
    Path(path): Path<TokenKeyPath>,
) -> Response {
    private_no_cache(revoke_token_by_key(&state, &identity, &path.key).await)
}

pub(crate) async fn delete_session_token(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
    Path(path): Path<TokenPath>,
) -> Response {
    private_no_cache(logout(&state, &identity, &path.token).await)
}

pub(super) async fn create_token(
    state: &AppState,
    client_ip: Option<IpAddr>,
    identity: &Identity,
    body: &[u8],
) -> Response {
    let username = match require_caller(identity, "token creation") {
        Ok(u) => u,
        Err(err) => return err.into_response(),
    };
    if let Err(retry_after) = state.inner.identity.login_rate_limiter.check(client_ip, &username) {
        return super::user_accounts::login_throttled_response(state, retry_after);
    }
    let request = match payload::parse_and_validate_token_request(body, token_store_unix_seconds())
    {
        Ok(req) => req,
        Err(err) => return err.into_response(),
    };
    if let Err(err) = verify_caller_password(state, client_ip, &username, request.password()).await
    {
        return err.into_response();
    }
    let new_token = match request.into_new_token(token_store_unix_seconds()) {
        Ok(token) => token,
        Err(err) => return err.into_response(),
    };
    let user_id = identity.user_id().expect("authenticated caller");
    match state.inner.identity.auth.tokens.issue_token(user_id, new_token).await {
        Ok((raw, record)) => json_response(StatusCode::OK, &create_token_response(&raw, &record)),
        Err(err) => err.into_response(),
    }
}

pub(super) async fn verify_caller_password(
    state: &AppState,
    client_ip: Option<IpAddr>,
    username: &str,
    password: Option<&str>,
) -> Result<(), RegistryError> {
    if let Some(hash) = state.inner.identity.auth.users.password_hash(username).await?
        && hash != pnpr_auth::NO_PASSWORD_HASH
    {
        let Some(password) = password else {
            state.inner.identity.login_rate_limiter.record_failure(client_ip, username);
            return Err(RegistryError::Unauthenticated {
                resource: "password required for token creation".to_string(),
            });
        };
        let _permit = state.inner.identity.login_concurrency.acquire().await;
        let valid = pnpr_auth::verify_bcrypt(password.to_string(), hash).await?;
        if !valid {
            state.inner.identity.login_rate_limiter.record_failure(client_ip, username);
            return Err(RegistryError::Unauthenticated {
                resource: "password verification".to_string(),
            });
        }
    }
    state.inner.identity.login_rate_limiter.record_success(client_ip, username);
    Ok(())
}

fn create_token_response(raw_token: &str, record: &TokenRecord) -> Value {
    let key = super::ecosystem::sha256_hex(raw_token.as_bytes());
    let mut obj = json!({
        "token": raw_token,
        "key": key,
        "user": record.user_id,
        "readonly": record.readonly,
        "cidr_whitelist": record.cidr_whitelist,
        "created": token_timestamp_iso(record.created_at),
        "updated": token_timestamp_iso(record.last_used_at),
    });
    if let Some(exp) = record.expires_at {
        obj["expires"] = json!(token_timestamp_iso(exp));
    }
    if let Some(ref name) = record.name {
        obj["name"] = json!(name);
    }
    if let Some(ref scopes) = record.scopes {
        obj["scopes"] = json!(scopes);
    }
    if let Some(ref kind) = record.kind {
        obj["kind"] = json!(kind);
    }
    obj
}

pub(super) async fn list_tokens(state: &AppState, identity: &Identity) -> Response {
    if let Err(err) = require_caller(identity, "token list") {
        return err.into_response();
    }
    let tokens = match state.inner.identity.auth.tokens.list_for_user(
        identity.user_id().expect("authenticated caller"),
    )
    .await
    {
        Ok(tokens) => tokens,
        Err(err) => return err.into_response(),
    };
    let objects: Vec<Value> = tokens
        .into_iter()
        .map(|(key, record)| token_response_object(&key, &record))
        .collect();
    json_response(StatusCode::OK, &json!({ "objects": objects, "urls": {} }))
}

pub(super) async fn revoke_token_by_key(
    state: &AppState,
    identity: &Identity,
    key: &str,
) -> Response {
    let username = match require_caller(identity, "token revocation") {
        Ok(username) => username,
        Err(err) => return err.into_response(),
    };
    match state.inner.identity.auth.tokens.find_by_key(key).await {
        Ok(Some(record)) if Some(record.user_id.as_str()) != identity.user_id() => {
            RegistryError::Forbidden {
                user: username,
                action: "revoke",
                resource: "this token".to_string(),
            }
            .into_response()
        }
        Ok(Some(_)) => match state.inner.identity.auth.tokens.revoke_by_key(key).await {
            Ok(Some(_)) => json_response(StatusCode::OK, &json!({ "ok": "token revoked" })),
            Ok(None) => not_found(),
            Err(err) => err.into_response(),
        },
        Ok(None) => not_found(),
        Err(err) => err.into_response(),
    }
}

pub(super) async fn logout(state: &AppState, identity: &Identity, raw_token: &str) -> Response {
    if let Err(err) = require_caller(identity, "logout") {
        return err.into_response();
    }
    if let Some(resp) = try_logout_session(state, identity, raw_token) {
        return resp;
    }
    logout_bearer_token(state, identity, raw_token).await
}

fn try_logout_session(state: &AppState, identity: &Identity, raw_token: &str) -> Option<Response> {
    match state.inner.identity.oidc.session(raw_token) {
        Ok(Some(owner)) if Some(owner.user_id.as_str()) == identity.user_id() => {
            state.inner.identity.oidc.revoke_session(raw_token);
            Some(json_response(StatusCode::OK, &json!({ "ok": true })))
        }
        Ok(Some(_)) => Some(
            RegistryError::Forbidden {
                user: identity
                    .username()
                    .expect("authenticated caller")
                    .to_string(),
                action: "revoke",
                resource: "this session".to_string(),
            }
            .into_response(),
        ),
        Err(err) => Some(err.into_response()),
        Ok(None) => None,
    }
}

async fn logout_bearer_token(state: &AppState, identity: &Identity, raw_token: &str) -> Response {
    let target_owner = match state.inner.identity.auth.tokens.lookup(raw_token).await {
        Ok(Some(owner)) => owner,
        Ok(None) => return not_found(),
        Err(err) => return err.into_response(),
    };
    if Some(target_owner.as_str()) != identity.user_id() {
        return RegistryError::Forbidden {
            user: identity
                .username()
                .expect("authenticated caller")
                .to_string(),
            action: "revoke",
            resource: "this token".to_string(),
        }
        .into_response();
    }
    match state.inner.identity.auth.tokens.revoke_by_raw(raw_token).await {
        Ok(Some(_)) => json_response(StatusCode::OK, &json!({ "ok": true })),
        Ok(None) => not_found(),
        Err(err) => err.into_response(),
    }
}

pub(super) fn token_response_object(key: &str, record: &pnpr_auth::TokenRecord) -> Value {
    let preview: String = key.chars().take(6).collect();
    let created = token_timestamp_iso(record.created_at);
    let updated = token_timestamp_iso(record.last_used_at);
    let mut obj = json!({
        "key": key,
        "token": preview,
        "user": record.user_id,
        "cidr_whitelist": record.cidr_whitelist,
        "readonly": record.readonly,
        "created": created,
        "updated": updated,
    });
    if let Some(exp) = record.expires_at {
        obj["expires"] = json!(token_timestamp_iso(exp));
    }
    if let Some(ref name) = record.name {
        obj["name"] = json!(name);
    }
    if let Some(ref scopes) = record.scopes {
        obj["scopes"] = json!(scopes);
    }
    if let Some(ref kind) = record.kind {
        obj["kind"] = json!(kind);
    }
    obj
}

pub(super) fn token_timestamp_iso(seconds: u64) -> String {
    iso_from_unix_millis(token_timestamp_millis(seconds))
}

pub(super) fn token_timestamp_millis(seconds: u64) -> i64 {
    const MILLIS_PER_SECOND: u64 = 1000;
    let max_seconds = i64::MAX as u64 / MILLIS_PER_SECOND;
    (seconds.min(max_seconds) * MILLIS_PER_SECOND) as i64
}

fn token_store_unix_seconds() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}
