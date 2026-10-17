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

#[derive(Default, Deserialize)]
struct CreateTokenPayload {
    password: Option<String>,
    #[serde(default)]
    readonly: bool,
    #[serde(default)]
    cidr_whitelist: Vec<String>,
    name: Option<String>,
    expires: Option<Value>,
    expires_in: Option<u64>,
    scopes: Option<Vec<String>>,
    kind: Option<String>,
}

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
    let payload = match parse_create_token_payload(body) {
        Ok(p) => p,
        Err(err) => return err.into_response(),
    };
    if let Err(err) = validate_token_payload(&payload) {
        return err.into_response();
    }
    let now = token_store_unix_seconds();
    let expires_at = match parse_token_expiry(&payload, now) {
        Ok(exp) => exp,
        Err(err) => return err.into_response(),
    };
    if let Err(err) =
        verify_caller_password(state, client_ip, &username, payload.password.as_deref()).await
    {
        return err.into_response();
    }
    let new_token = pnpr_auth::NewToken::new()
        .with_readonly(payload.readonly)
        .with_cidr_whitelist(payload.cidr_whitelist)
        .with_name(payload.name)
        .with_expires_at(expires_at)
        .with_scopes(payload.scopes)
        .with_kind(payload.kind);
    match state.inner.identity.auth.tokens.issue_token(&username, new_token).await {
        Ok((raw, record)) => json_response(StatusCode::OK, &create_token_response(&raw, &record)),
        Err(err) => err.into_response(),
    }
}

fn validate_token_payload(payload: &CreateTokenPayload) -> Result<(), RegistryError> {
    validate_cidr_whitelist(&payload.cidr_whitelist)?;
    if let Some(kind) = payload.kind.as_deref()
        && kind.chars().count() > 64
    {
        return Err(RegistryError::BadRequest {
            reason: "kind must be at most 64 characters".to_string(),
        });
    }
    Ok(())
}

fn parse_create_token_payload(body: &[u8]) -> Result<CreateTokenPayload, RegistryError> {
    if body.is_empty() {
        return Ok(CreateTokenPayload::default());
    }
    serde_json::from_slice(body).map_err(RegistryError::Json)
}

fn validate_cidr_whitelist(whitelist: &[String]) -> Result<(), RegistryError> {
    for entry in whitelist {
        if pnpr_config::IpNetwork::parse(entry.trim()).is_err() {
            return Err(RegistryError::BadRequest {
                reason: format!("invalid CIDR whitelist entry: {entry}"),
            });
        }
    }
    Ok(())
}

fn parse_token_expiry(
    payload: &CreateTokenPayload,
    now: u64,
) -> Result<Option<u64>, RegistryError> {
    if let Some(sec) = payload.expires_in {
        if sec == 0 {
            return Err(RegistryError::BadRequest {
                reason: "expires_in must be greater than 0".to_string(),
            });
        }
        return Ok(Some(now.saturating_add(sec)));
    }
    let Some(val) = &payload.expires else {
        return Ok(None);
    };
    parse_expiry_value(val, now)
}

fn parse_expiry_value(val: &Value, now: u64) -> Result<Option<u64>, RegistryError> {
    match val {
        Value::Number(n) => {
            let n = n
                .as_u64()
                .ok_or_else(|| RegistryError::BadRequest {
                    reason: "invalid expiry timestamp".to_string(),
                })?;
            resolve_future_expiry(n, now)
        }
        Value::String(s) => parse_expiry_string(s, now),
        Value::Null => Ok(None),
        _ => Err(RegistryError::BadRequest { reason: "invalid expiry value".to_string() }),
    }
}

fn parse_expiry_string(raw: &str, now: u64) -> Result<Option<u64>, RegistryError> {
    if let Ok(n) = raw.parse::<u64>() {
        resolve_future_expiry(n, now)
    } else if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        let exp = dt.timestamp().max(0) as u64;
        if exp <= now {
            return Err(RegistryError::BadRequest {
                reason: "expiry must be in the future".to_string(),
            });
        }
        Ok(Some(exp))
    } else {
        Err(RegistryError::BadRequest { reason: format!("unsupported expiry format: {raw}") })
    }
}

fn resolve_future_expiry(raw: u64, now: u64) -> Result<Option<u64>, RegistryError> {
    let exp = if raw < 100_000_000 {
        now.saturating_add(raw)
    } else if raw > 1_000_000_000_000 {
        raw / 1000
    } else {
        raw
    };
    if exp <= now {
        return Err(RegistryError::BadRequest {
            reason: "expiry must be in the future".to_string(),
        });
    }
    Ok(Some(exp))
}

async fn verify_caller_password(
    state: &AppState,
    client_ip: Option<IpAddr>,
    username: &str,
    password: Option<&str>,
) -> Result<(), RegistryError> {
    if let Some(hash) = state.inner.identity.auth.users.password_hash(username).await? {
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
                resource: "invalid password".to_string(),
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
        "user": record.username,
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
    let username = match require_caller(identity, "token list") {
        Ok(username) => username,
        Err(err) => return err.into_response(),
    };
    let tokens = match state.inner.identity.auth.tokens.list_for_user(&username).await {
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
        Ok(Some(record)) if record.username != username => RegistryError::Forbidden {
            user: username,
            action: "revoke",
            resource: "this token".to_string(),
        }
        .into_response(),
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
    let username = match require_caller(identity, "logout") {
        Ok(username) => username,
        Err(err) => return err.into_response(),
    };
    if let Some(resp) = try_logout_session(state, &username, raw_token) {
        return resp;
    }
    logout_bearer_token(state, &username, raw_token).await
}

fn try_logout_session(state: &AppState, username: &str, raw_token: &str) -> Option<Response> {
    match state.inner.identity.oidc.session(raw_token) {
        Ok(Some(owner)) if owner.username == username => {
            state.inner.identity.oidc.revoke_session(raw_token);
            Some(json_response(StatusCode::OK, &json!({ "ok": true })))
        }
        Ok(Some(_)) => Some(
            RegistryError::Forbidden {
                user: username.to_string(),
                action: "revoke",
                resource: "this session".to_string(),
            }
            .into_response(),
        ),
        Err(err) => Some(err.into_response()),
        Ok(None) => None,
    }
}

async fn logout_bearer_token(state: &AppState, username: &str, raw_token: &str) -> Response {
    let target_owner = match state.inner.identity.auth.tokens.lookup(raw_token).await {
        Ok(Some(owner)) => owner,
        Ok(None) => return not_found(),
        Err(err) => return err.into_response(),
    };
    if target_owner != username {
        return RegistryError::Forbidden {
            user: username.to_string(),
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
        "user": record.username,
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
