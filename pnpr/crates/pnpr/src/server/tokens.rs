mod payload;

use payload::CredentialLimits;

use std::net::IpAddr;

use axum::{
    extract::{Path, State},
    http::HeaderMap,
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
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let ip = client_ip.map(|axum::extract::Extension(super::ClientIp(ip))| ip);
    let credential = super::single_authorization_header(&headers)
        .ok()
        .flatten()
        .and_then(super::authentication::token_credentials);
    let caller = Caller { identity: &identity, credential: credential.as_deref() };
    private_no_cache(create_token(&state, ip, &caller, &body).await)
}

/// Who calls the token API, and the raw credential they authenticated with.
pub(super) struct Caller<'a> {
    pub(super) identity: &'a Identity,
    pub(super) credential: Option<&'a str>,
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
    caller: &Caller<'_>,
    body: &[u8],
) -> Response {
    let username = match require_caller(caller.identity, "token creation") {
        Ok(u) => u,
        Err(err) => return err.into_response(),
    };
    if let Err(retry_after) = state.inner.identity.login_rate_limiter.check(client_ip, &username) {
        return super::user_accounts::login_throttled_response(state, retry_after);
    }
    let _permit = state.inner.identity.login_concurrency.acquire().await;
    if let Err(retry_after) = state.inner.identity.login_rate_limiter.check(client_ip, &username) {
        return super::user_accounts::login_throttled_response(state, retry_after);
    }
    create_verified_token(state, client_ip, caller, body).await
        .unwrap_or_else(IntoResponse::into_response)
}

async fn create_verified_token(
    state: &AppState,
    client_ip: Option<IpAddr>,
    caller: &Caller<'_>,
    body: &[u8],
) -> Result<Response, RegistryError> {
    let username = caller.identity.username().expect("authenticated caller");
    let user_id = caller.identity.user_id().expect("authenticated caller");
    let mut request = payload::parse_and_validate_token_request(body, pnpr_auth::unix_millis())?;
    let expected_revision = state.inner.identity.auth
        .active_account_revision(user_id)
        .await?
        .ok_or_else(|| RegistryError::Unauthenticated { resource: format!("user {username:?}") })?;
    let verified_credential =
        verify_caller_password(state, client_ip, caller.identity, request.password()).await?;
    let now = pnpr_auth::unix_millis();
    request = request
        .expiring_by_default(default_token_expiry(state, now))
        .expiring_by(latest_token_expiry(state, now));
    if matches!(verified_credential, pnpr_auth::AccountCredential::None) {
        request = request.within(credential_limits(state, caller.credential).await?)?;
    }
    let new_token = request.into_new_token(pnpr_auth::unix_millis())?;
    let (raw, record) = state.inner.identity.auth.issue_token_if_credential_current(
        username,
        user_id,
        &verified_credential,
        expected_revision,
        new_token,
    )
    .await?;
    let key = super::ecosystem::sha256_hex(raw.as_bytes());
    let actor = crate::audit::Actor::of(caller.identity);
    crate::audit::audit!(
        actor,
        "token.create",
        token_key = %key,
        readonly = record.readonly,
        "token created",
    );
    Ok(json_response(StatusCode::OK, &create_token_response(&raw, &key, &record)))
}

/// When a token issued at `now` without a requested expiry expires: after
/// `auth.tokens.defaultLifetime`, but no later than `auth.tokens.maxLifetime`.
pub(super) fn unrequested_token_expiry(state: &AppState, now: u64) -> u64 {
    let default = default_token_expiry(state, now);
    latest_token_expiry(state, now).map_or(default, |latest| latest.min(default))
}

fn default_token_expiry(state: &AppState, now: u64) -> u64 {
    payload::expiry_after(state.inner.config.identity.auth.tokens.default_lifetime, now)
}

/// When a token issued at `now` expires at the latest under
/// `auth.tokens.maxLifetime`. `None` when no maximum is configured.
fn latest_token_expiry(state: &AppState, now: u64) -> Option<u64> {
    let max_lifetime = state.inner.config.identity.auth.tokens.max_lifetime?;
    Some(payload::expiry_after(max_lifetime, now))
}

/// The limits of the browser session or token that `credential` carries.
/// Without a password, a new token is held to them.
async fn credential_limits(
    state: &AppState,
    credential: Option<&str>,
) -> Result<CredentialLimits, RegistryError> {
    let unsupported = || RegistryError::Unauthenticated {
        resource: "token creation without a password from this credential".to_string(),
    };
    let raw = credential.ok_or_else(unsupported)?;
    if let Some(expires) = state.inner.identity.oidc.session_expires(raw) {
        const MILLIS_PER_SECOND: u64 = 1_000;
        let expires_at = u64::try_from(expires)
            .ok()
            .and_then(|seconds| seconds.checked_mul(MILLIS_PER_SECOND));
        return Ok(CredentialLimits { cidr_whitelist: Vec::new(), expires_at });
    }
    let record =
        state.inner.identity.auth.tokens.lookup_record(raw).await?.ok_or_else(unsupported)?;
    Ok(CredentialLimits { cidr_whitelist: record.cidr_whitelist, expires_at: record.expires_at })
}

pub(super) async fn verify_caller_password(
    state: &AppState,
    client_ip: Option<IpAddr>,
    identity: &Identity,
    password: Option<&str>,
) -> Result<pnpr_auth::AccountCredential, RegistryError> {
    let username = identity.username().expect("authenticated caller");
    let credential = state.inner.identity.auth.users
        .credential(username)
        .await?
        .ok_or_else(|| RegistryError::Unauthenticated {
            resource: "password verification".to_string(),
        })?;
    if let Err(error) = verify_account_credential(username, password, &credential).await {
        if matches!(error, RegistryError::Unauthenticated { .. }) {
            let actor = crate::audit::Actor::of(identity);
            super::user_accounts::record_login_failure(state, client_ip, actor, username);
        }
        return Err(error);
    }
    state.inner.identity.login_rate_limiter.record_success(client_ip, username);
    Ok(credential)
}

async fn verify_account_credential(
    username: &str,
    password: Option<&str>,
    credential: &pnpr_auth::AccountCredential,
) -> Result<(), RegistryError> {
    if matches!(credential, pnpr_auth::AccountCredential::None) {
        return Ok(());
    }
    let password = password.ok_or_else(|| RegistryError::Unauthenticated {
        resource: "password required for token creation".to_string(),
    })?;
    match pnpr_auth::verify_returning_user(
        username,
        password,
        credential,
        pnpr_auth::DEFAULT_BCRYPT_COST,
    )
    .await
    {
        Ok(_) => Ok(()),
        Err(RegistryError::Unauthenticated { .. }) => {
            Err(RegistryError::Unauthenticated { resource: "password verification".to_string() })
        }
        Err(error) => Err(error),
    }
}

fn create_token_response(raw_token: &str, key: &str, record: &TokenRecord) -> Value {
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
    if let Some(kind) = record.kind {
        obj["kind"] = json!(kind.as_str());
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
            Ok(Some(_)) => {
                audit_token_revoke(identity, key);
                json_response(StatusCode::OK, &json!({ "ok": "token revoked" }))
            }
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
        Ok(Some(_)) => {
            audit_token_revoke(identity, &super::ecosystem::sha256_hex(raw_token.as_bytes()));
            json_response(StatusCode::OK, &json!({ "ok": true }))
        }
        Ok(None) => not_found(),
        Err(err) => err.into_response(),
    }
}

fn audit_token_revoke(identity: &Identity, key: &str) {
    let actor = crate::audit::Actor::of(identity);
    crate::audit::audit!(
        actor,
        "token.revoke",
        token_key = %key,
        "token revoked",
    );
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
    if let Some(kind) = record.kind {
        obj["kind"] = json!(kind.as_str());
    }
    obj
}

pub(super) fn token_timestamp_iso(millis: u64) -> String {
    iso_from_unix_millis(i64::try_from(millis).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests;
