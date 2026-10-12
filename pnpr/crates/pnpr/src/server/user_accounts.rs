use super::{
    AppState, AuthedCaller, Body, Deserialize, Identity, Path, RegistryError, Response, State,
    StatusCode, TargetRegistry, UpsertOutcome, Value, header, iso_from_unix_millis, json,
    json_response, not_found, private_no_cache, require_caller,
};
use axum::response::IntoResponse;

pub(super) async fn get_whoami(
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
) -> Response {
    private_no_cache(serve_whoami(&identity))
}

/// `PUT /-/user/org.couchdb.user:{name}` — adduser / login. Authenticates
/// from the request body, not the caller's existing identity.
pub(super) async fn put_login(
    State(state): State<AppState>,
    TargetRegistry(_): TargetRegistry,
    Path(path): Path<UserPath>,
    body: axum::body::Bytes,
) -> Response {
    match path.user.strip_prefix("org.couchdb.user:") {
        Some(name) => add_user(&state, name, &body).await,
        None => not_found(),
    }
}

pub(super) async fn delete_session_token(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
    Path(path): Path<TokenPath>,
) -> Response {
    private_no_cache(logout(&state, &identity, &path.token).await)
}

pub(super) async fn get_profile(
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
) -> Response {
    private_no_cache(serve_profile(&identity))
}

pub(super) async fn get_token_list(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
) -> Response {
    private_no_cache(list_tokens(&state, &identity).await)
}

pub(super) async fn delete_token_by_key(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
    Path(path): Path<TokenKeyPath>,
) -> Response {
    private_no_cache(revoke_token_by_key(&state, &identity, &path.key).await)
}

/// The account routes capture their own parameter alongside the optional
/// `{registry}`, so each needs a named shape rather than a bare `Path<String>`:
/// the prefixed registration captures two segments and a single-value `Path`
/// would refuse to deserialize it.
#[derive(Deserialize)]
pub(super) struct UserPath {
    pub(super) user: String,
}

#[derive(Deserialize)]
pub(super) struct TokenPath {
    pub(super) token: String,
}

#[derive(Deserialize)]
pub(super) struct TokenKeyPath {
    pub(super) key: String,
}

/// Add a new user or log in an existing one. Mirrors verdaccio's
/// `/-/user/org.couchdb.user/:name` behavior:
///
/// * unknown user → create + return 201 with `{ ok, token }`.
/// * existing user, password matches → return 201 with `{ ok, token }`.
/// * existing user, password wrong → 401.
pub(super) async fn add_user(state: &AppState, name: &str, body: &[u8]) -> Response {
    // axum's `Path` extractor already percent-decodes path segments
    // (`%2F` → `/`, `%40` → `@`, etc.), so we use `name` verbatim.
    let body: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(err) => return RegistryError::Json(err).into_response(),
    };
    let body_name = body
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("");
    if body_name != name {
        return RegistryError::BadRequest {
            reason: format!("username in URL ({name:?}) does not match body ({body_name:?})"),
        }
        .into_response();
    }
    let Some(password) = body.get("password").and_then(Value::as_str) else {
        return RegistryError::BadRequest { reason: "missing password".to_string() }.into_response(
        );
    };

    let (outcome, username, token) = match log_in(state, name, password).await {
        Ok(login) => login,
        Err(err) => return err.into_response(),
    };
    let ok_msg = match outcome {
        UpsertOutcome::Created => format!("user '{username}' created"),
        UpsertOutcome::LoggedIn => format!("you are authenticated as '{username}'"),
    };
    let body =
        json!({ "ok": ok_msg, "token": token, "id": format!("org.couchdb.user:{username}") });
    let bytes = serde_json::to_vec(&body).expect("static-shape JSON serializes");
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

/// Verify or register `name` and issue its token. Returns the outcome, the
/// stored username, and the raw token.
async fn log_in(
    state: &AppState,
    name: &str,
    password: &str,
) -> Result<(UpsertOutcome, String, String), RegistryError> {
    if super::scim::is_deprovisioned(state, name)? {
        return Err(deprovisioned(name));
    }
    let users = &state.inner.identity.auth.users;
    let verified = users.password_hash(name).await?;
    if verified.is_none() {
        // Tokens a removed account left behind must not reach the account
        // this login may create, not even while it is being created.
        super::user_admin::revoke_all_tokens(state, name).await?;
    }
    let (outcome, username) = users.add_or_login(name, password).await?;
    let token = state.inner.identity.auth.tokens.issue(&username).await?;
    let refused = match super::scim::is_deprovisioned(state, &username) {
        Ok(false) => None,
        Ok(true) => Some(deprovisioned(&username)),
        Err(err) => Some(err),
    };
    if let Some(err) = refused {
        undo_login(state, &username, &token, outcome).await?;
        return Err(err);
    }
    if still_the_same_account(state, &username, password, verified.as_deref()).await? {
        return Ok((outcome, username, token));
    }
    state.inner.identity.auth.tokens.revoke_by_raw(&token).await?;
    Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") })
}

fn deprovisioned(name: &str) -> RegistryError {
    RegistryError::Forbidden {
        user: name.to_string(),
        action: "sign in to",
        resource: "this account, which an identity provider deprovisioned".to_string(),
    }
}

/// Take back the token a login issued, and the account it created, when a
/// SCIM deprovisioning of the user landed while the login ran, or the SCIM
/// directory could no longer be read.
async fn undo_login(
    state: &AppState,
    username: &str,
    token: &str,
    outcome: UpsertOutcome,
) -> Result<(), RegistryError> {
    let auth = &state.inner.identity.auth;
    auth.tokens.revoke_by_raw(token).await?;
    if matches!(outcome, UpsertOutcome::Created) {
        auth.users.delete_user(username).await?;
    }
    Ok(())
}

/// Whether the account a login verified, or created, is still the one stored,
/// checked once the login's token exists. An admin may remove the account, or
/// remove it and create it again, between the check and the issue, after the
/// removal revoked the account's tokens.
///
/// A login that verified a stored hash (`verified`) needs that hash to still
/// be stored. The hash is salted, so a recreated account or a new password
/// never matches. A login that created the account has no hash to compare, so
/// the stored account must accept the password that created it.
async fn still_the_same_account(
    state: &AppState,
    username: &str,
    password: &str,
    verified: Option<&str>,
) -> Result<bool, RegistryError> {
    let current = state.inner.identity.auth.users.password_hash(username).await?;
    match (verified, current) {
        (Some(verified), Some(current)) => Ok(verified == current),
        (None, Some(current)) => pnpr_auth::verify_bcrypt(password.to_string(), current).await,
        (_, None) => Ok(false),
    }
}

/// `GET /-/whoami` — return the username of the caller, or 401 if
/// the request is anonymous. `npm whoami` reads this. The check is
/// pure auth: no per-package policy applies, so anonymous always
/// gets 401 even when `$all` would let it through for packument
/// reads.
pub(super) fn serve_whoami(identity: &Identity) -> Response {
    let username = match require_caller(identity, "user identity") {
        Ok(username) => username,
        Err(err) => return err.into_response(),
    };
    json_response(StatusCode::OK, &json!({ "username": username }))
}

/// `GET /-/npm/v1/user` — return the profile of the authenticated
/// caller. `npm profile get` reads this. pnpr doesn't track email,
/// 2FA, or anything beyond the username; the absent fields surface
/// as their zero-value defaults so the npm CLI's table renderer
/// doesn't choke on a missing key.
pub(super) fn serve_profile(identity: &Identity) -> Response {
    let username = match require_caller(identity, "user profile") {
        Ok(username) => username,
        Err(err) => return err.into_response(),
    };
    json_response(
        StatusCode::OK,
        &json!({
            "name": username,
            "email": "",
            "email_verified": false,
            "tfa": false,
            "fullname": "",
            "cidr_whitelist": null,
        }),
    )
}

/// `GET /-/npm/v1/tokens` — list every bearer token issued to the
/// authenticated caller. Returns the npm-CLI-compatible wrapper
/// (`{ objects, urls }`) so `npm token list` parses it cleanly. The
/// raw token itself is never persisted; the `token` field surfaces
/// the leading 6 hex characters of the key as a preview, matching
/// what verdaccio does when it can't reconstruct the original.
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

/// `DELETE /-/npm/v1/tokens/token/:key` — revoke a token by its
/// listing-side key. The caller must be the owner of the token
/// (anonymous is 401, a different authenticated user is 403); an
/// unknown key returns 404. `npm token revoke` calls this with the
/// `key` it pulled from [`list_tokens`].
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

/// `DELETE /-/user/token/:tok` — npm logout. The path holds the raw
/// bearer token (npm sends it verbatim alongside an
/// `Authorization: Bearer <tok>` header). We require authentication
/// and require that the auth identifies the same user who owns the
/// token being deleted.
pub(super) async fn logout(state: &AppState, identity: &Identity, raw_token: &str) -> Response {
    let username = match require_caller(identity, "logout") {
        Ok(username) => username,
        Err(err) => return err.into_response(),
    };
    match state.inner.identity.oidc.session(raw_token) {
        Ok(Some(owner)) if owner.username == username => {
            state.inner.identity.oidc.revoke_session(raw_token);
            return json_response(StatusCode::OK, &json!({ "ok": true }));
        }
        Ok(Some(_)) => {
            return RegistryError::Forbidden {
                user: username,
                action: "revoke",
                resource: "this session".to_string(),
            }
            .into_response();
        }
        Err(err) => return err.into_response(),
        Ok(None) => {}
    }
    let target_owner = match state.inner.identity.auth.tokens.lookup(raw_token).await {
        Ok(Some(owner)) => owner,
        Ok(None) => return not_found(),
        Err(err) => return err.into_response(),
    };
    if target_owner != username {
        return RegistryError::Forbidden {
            user: username,
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
    json!({
        "key": key,
        "token": preview,
        "user": record.username,
        "cidr_whitelist": record.cidr_whitelist,
        "readonly": record.readonly,
        "created": created,
        "updated": updated,
    })
}

pub(super) fn token_timestamp_iso(seconds: u64) -> String {
    iso_from_unix_millis(token_timestamp_millis(seconds))
}

pub(super) fn token_timestamp_millis(seconds: u64) -> i64 {
    const MILLIS_PER_SECOND: u64 = 1000;
    let max_seconds = i64::MAX as u64 / MILLIS_PER_SECOND;
    (seconds.min(max_seconds) * MILLIS_PER_SECOND) as i64
}
