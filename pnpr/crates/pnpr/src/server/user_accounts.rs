use super::{
    AppState, AuthedCaller, Body, Deserialize, Identity, Path, RegistryError, Response, State,
    StatusCode, TargetRegistry, UpsertOutcome, Value, header, json, json_response, not_found,
    private_no_cache, require_caller,
};
use axum::response::IntoResponse;

pub(crate) async fn get_whoami(
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
) -> Response {
    private_no_cache(serve_whoami(&identity))
}

/// `PUT /-/user/org.couchdb.user:{name}` — adduser / login. Authenticates
/// from the request body, not the caller's existing identity.
pub(crate) async fn put_login(
    State(state): State<AppState>,
    TargetRegistry(_): TargetRegistry,
    client_ip: Option<axum::extract::Extension<super::ClientIp>>,
    Path(path): Path<UserPath>,
    body: axum::body::Bytes,
) -> Response {
    let Some(name) = path.user.strip_prefix("org.couchdb.user:") else {
        return not_found();
    };
    let ip_address = client_ip.map(|axum::extract::Extension(super::ClientIp(ip))| ip);
    if let Err(retry_after) = state.inner.identity.login_rate_limiter.check(ip_address, name) {
        return login_throttled_response(&state, retry_after);
    }
    add_user(&state, ip_address, name, &body).await
}

pub(crate) fn login_throttled_response(state: &AppState, retry_after: u64) -> Response {
    state.inner.metrics.record_login_throttled();
    (
        StatusCode::TOO_MANY_REQUESTS,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::RETRY_AFTER, &retry_after.to_string()),
        ],
        json!({
            "error": "too many failed login attempts, please try again later"
        })
        .to_string(),
    )
        .into_response()
}

pub(crate) async fn get_profile(
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
) -> Response {
    private_no_cache(serve_profile(&identity))
}

/// The account routes capture their own parameter alongside the optional
/// `{registry}`, so each needs a named shape rather than a bare `Path<String>`:
/// the prefixed registration captures two segments and a single-value `Path`
/// would refuse to deserialize it.
#[derive(Deserialize)]
pub(super) struct UserPath {
    pub(super) user: String,
}

/// Add a new user or log in an existing one. Mirrors verdaccio's
/// `/-/user/org.couchdb.user/:name` behavior:
///
/// * unknown user → create + return 201 with `{ ok, token }`.
/// * existing user, password matches → return 201 with `{ ok, token }`.
/// * existing user, password wrong → 401.
fn parse_registration_password(name: &str, body: &[u8]) -> Result<String, RegistryError> {
    let body: Value = serde_json::from_slice(body).map_err(RegistryError::Json)?;
    let body_name = body
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("");
    if body_name != name {
        return Err(RegistryError::BadRequest {
            reason: format!("username in URL ({name:?}) does not match body ({body_name:?})"),
        });
    }
    let Some(password) = body.get("password").and_then(Value::as_str) else {
        return Err(RegistryError::BadRequest { reason: "missing password".to_string() });
    };
    if password.is_empty() {
        return Err(RegistryError::BadRequest { reason: "password cannot be empty".to_string() });
    }
    if password.len() > pnpr_auth::MAX_PASSWORD_BYTES {
        return Err(RegistryError::BadRequest {
            reason: format!("password cannot exceed {} bytes", pnpr_auth::MAX_PASSWORD_BYTES),
        });
    }
    Ok(password.to_string())
}

pub(super) async fn add_user(
    state: &AppState,
    client_ip: Option<std::net::IpAddr>,
    name: &str,
    body: &[u8],
) -> Response {
    let password = match parse_registration_password(name, body) {
        Ok(pw) => pw,
        Err(err) => return err.into_response(),
    };

    let _permit = state.inner.identity.login_concurrency.acquire().await;
    if let Err(retry_after) = state.inner.identity.login_rate_limiter.check(client_ip, name) {
        return login_throttled_response(state, retry_after);
    }
    let (outcome, username, token) = match log_in(state, name, &password).await {
        Ok(login) => {
            state.inner.identity.login_rate_limiter.record_success(client_ip, name);
            login
        }
        Err(err) => {
            if matches!(
                err,
                RegistryError::Unauthenticated { .. } | RegistryError::Forbidden { .. },
            ) {
                state.inner.identity.login_rate_limiter.record_failure(client_ip, name);
            }
            return err.into_response();
        }
    };
    user_upsert_response(outcome, &username, &token)
}

fn user_upsert_response(outcome: UpsertOutcome, username: &str, token: &str) -> Response {
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
