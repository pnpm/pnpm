use super::{
    AppState, AuthedCaller, Body, Deserialize, Identity, Path, RegistryError, Response, State,
    StatusCode, TargetRegistry, UpsertOutcome, Value, header, json, json_response,
    private_no_cache, require_caller,
};
use axum::response::IntoResponse;

pub(crate) async fn get_whoami(
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
) -> Response {
    private_no_cache(serve_whoami(&identity))
}

fn strip_couchdb_user_prefix(user: &str) -> &str {
    user.strip_prefix("org.couchdb.user:").unwrap_or(user)
}

/// `PUT /-/user/{user}` — adduser / login. Accepts bare `/-/user/{name}` and
/// legacy `/-/user/org.couchdb.user:{name}`. Authenticates from the request
/// body, not the caller's existing identity.
pub(crate) async fn put_login(
    State(state): State<AppState>,
    TargetRegistry(_): TargetRegistry,
    client_ip: Option<axum::extract::Extension<super::ClientIp>>,
    Path(path): Path<UserPath>,
    body: axum::body::Bytes,
) -> Response {
    let name = strip_couchdb_user_prefix(&path.user);
    let ip_address = client_ip.map(|axum::extract::Extension(super::ClientIp(ip))| ip);
    let mut response = match state.inner.identity.login_rate_limiter.check(ip_address, name) {
        Err(retry_after) => login_throttled_response(&state, retry_after),
        Ok(()) => add_user(&state, ip_address, name, &body).await,
    };
    response.extensions_mut().insert(super::authentication::PasswordAuthentication);
    response
}

pub(crate) fn login_throttled_response(state: &AppState, retry_after: u64) -> Response {
    state.inner.metrics.record_login_throttled();
    let mut response = super::routing::npm_errors::json_error(
        StatusCode::TOO_MANY_REQUESTS,
        "too many failed login attempts, please try again later",
    );
    response
        .headers_mut()
        .insert(
            header::RETRY_AFTER,
            retry_after
                .to_string()
                .parse()
                .expect("integer is a valid header value"),
        );
    response
}

pub(crate) async fn get_profile(
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(_): TargetRegistry,
) -> Response {
    private_no_cache(serve_profile(&identity))
}

/// `GET /-/user/{user}` — fetch a user document. Accepts bare `/-/user/{name}`
/// and legacy `/-/user/org.couchdb.user:{name}`. Used by `npm owner add` to
/// verify that the prospective maintainer exists before mutating the package
/// document.
pub(crate) async fn get_user(
    State(state): State<AppState>,
    TargetRegistry(_): TargetRegistry,
    Path(path): Path<UserPath>,
) -> Response {
    let name = strip_couchdb_user_prefix(&path.user);
    private_no_cache(match active_account_id(&state, name).await {
        Ok(Some(id)) => json_response(
            StatusCode::OK,
            &json!({
                "_id": format!("org.couchdb.user:{name}"),
                "id": id,
                "name": name,
                "email": "",
                "roles": [],
            }),
        ),
        Ok(None) => super::routing::npm_errors::json_error(StatusCode::NOT_FOUND, "User not found"),
        Err(err) => err.into_response(),
    })
}

async fn active_account_id(state: &AppState, name: &str) -> Result<Option<String>, RegistryError> {
    let Some(id) = state.inner.identity.auth.users.account_id(name).await? else {
        return Ok(None);
    };
    Ok((!super::scim::is_deprovisioned(state, &id)?).then_some(id))
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
    let (outcome, username, id, token) = match log_in(state, name, &password).await {
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
    if matches!(outcome, UpsertOutcome::Created) {
        tracing::info!(
            target: "pnpr::audit",
            action = "user.register",
            actor = %username,
            user = %username,
            "user account registered",
        );
    }
    user_upsert_response(outcome, &username, &id, &token)
}

fn user_upsert_response(outcome: UpsertOutcome, username: &str, id: &str, token: &str) -> Response {
    let ok_msg = match outcome {
        UpsertOutcome::Created => format!("user '{username}' created"),
        UpsertOutcome::LoggedIn | _ => format!("you are authenticated as '{username}'"),
    };
    let body = json!({ "ok": ok_msg, "token": token, "id": id });
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
) -> Result<(UpsertOutcome, String, String, String), RegistryError> {
    let users = &state.inner.identity.auth.users;
    if let Some(id) = users.account_id(name).await?
        && super::scim::is_deprovisioned(state, &id)?
    {
        return Err(deprovisioned(name));
    }
    let (outcome, id) = users.add_or_login(name, password).await?;
    let token = state.inner.identity.auth.tokens.issue(&id).await?;
    if super::scim::is_deprovisioned(state, &id)? {
        state.inner.identity.auth.tokens.revoke_by_raw(&token).await?;
        return Err(deprovisioned(name));
    }
    let Some(current_name) = users.account_name(&id).await? else {
        state.inner.identity.auth.tokens.revoke_by_raw(&token).await?;
        return Err(RegistryError::Unauthenticated { resource: format!("account {id:?}") });
    };
    Ok((outcome, current_name, id, token))
}

fn deprovisioned(name: &str) -> RegistryError {
    RegistryError::Forbidden {
        user: name.to_string(),
        action: "sign in to",
        resource: "this account, which an identity provider deprovisioned".to_string(),
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
