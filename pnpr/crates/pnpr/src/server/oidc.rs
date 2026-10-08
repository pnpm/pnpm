use super::{AppState, private_no_cache};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use pnpr_auth::oidc::{LoginReturn, LoginSession};
use pnpr_error::RegistryError;
use serde::Deserialize;
use serde_json::json;

/// The web UI's page that redeems a handoff code, relative to
/// `/-/oidc/{provider}/callback`.
const UI_HANDOFF_PAGE: &str = "../../ui/sign-in/oidc";

#[derive(Deserialize)]
pub(super) struct LoginQuery {
    #[serde(rename = "return")]
    returns_to: Option<String>,
}

pub(super) async fn login(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(query): Query<LoginQuery>,
) -> Response {
    let returns_to = match query.returns_to.as_deref() {
        None => LoginReturn::Token,
        Some("ui") => LoginReturn::Ui,
        Some(_) => {
            return protect(
                RegistryError::BadRequest { reason: "return must be ui".to_string() }
                    .into_response(),
            );
        }
    };
    let response = match state.inner.identity.oidc.start(&provider, returns_to).await {
        Ok(start) => {
            let mut response = Redirect::to(&start.url).into_response();
            let cookie = format!(
                "__Host-pnpr-oidc-{}={}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=300",
                start.state, start.browser_secret,
            );
            match HeaderValue::from_str(&cookie) {
                Ok(cookie) => {
                    response.headers_mut().insert(header::SET_COOKIE, cookie);
                    response
                }
                Err(_) => RegistryError::Internal { reason: "invalid OIDC cookie".to_string() }
                    .into_response(),
            }
        }
        Err(err) => err.into_response(),
    };
    protect(response)
}

#[derive(Deserialize)]
pub(super) struct Callback {
    state: String,
    code: Option<String>,
}

pub(super) async fn callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(query): Query<Callback>,
    headers: HeaderMap,
) -> Response {
    if query.state.len() > 128
        || query.state.is_empty()
        || !query.state
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return protect(
            RegistryError::BadRequest { reason: "invalid OIDC state".to_string() }.into_response(),
        );
    }
    let cookie_name = format!("__Host-pnpr-oidc-{}", query.state);
    let response = if let Some(secret) = browser_secret(&headers, &cookie_name) {
        match state.inner.identity.oidc.finish(
            &provider,
            &query.state,
            secret,
            query.code.as_deref().unwrap_or(""),
        )
        .await
        {
            Ok(session) => signed_in(&state, session),
            Err(err) => err.into_response(),
        }
    } else {
        RegistryError::Unauthenticated { resource: "OIDC browser session".to_string() }
            .into_response()
    };
    let mut response = protect(response);
    let cookie = format!("{cookie_name}=; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=0");
    if let Ok(cookie) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    response
}

/// Where the browser lands after `session` signed in: the web UI with a
/// handoff code, or a page that shows the token.
fn signed_in(state: &AppState, session: LoginSession) -> Response {
    if session.returns_to == LoginReturn::Ui {
        return match state.inner.identity.oidc.hand_off(session) {
            Ok(code) => Redirect::to(&format!("{UI_HANDOFF_PAGE}?code={code}")).into_response(),
            Err(err) => err.into_response(),
        };
    }
    let token = session.token;
    let expires = session.expires;
    let message = format!(
        "Signed in to pnpr. Use this token as your registry's _authToken.\n\n{token}\n\nExpires at Unix time {expires}.\n",
    );
    (StatusCode::OK, message).into_response()
}

#[derive(Deserialize)]
pub(super) struct HandoffRequest {
    code: String,
}

/// `POST /-/oidc/handoff`: trades the one-time code that a sign-in started
/// with `return=ui` redirected to the web UI for the session token.
pub(super) async fn redeem_handoff(
    State(state): State<AppState>,
    Json(request): Json<HandoffRequest>,
) -> Response {
    let response = match state.inner.identity.oidc.redeem_handoff(&request.code) {
        Ok(session) => {
            Json(json!({ "token": session.token, "expires": session.expires })).into_response()
        }
        Err(err) => err.into_response(),
    };
    protect(response)
}

/// `GET /-/pnpr/v0/sign-in`: the OIDC providers that offer browser sign-in.
pub(super) async fn sign_in_methods(State(state): State<AppState>) -> Response {
    let providers: Vec<_> = state.inner.config.identity.auth.oidc
        .iter()
        .filter(|provider| provider.login.is_some())
        .map(|provider| json!({ "name": provider.name }))
        .collect();
    private_no_cache(Json(json!({ "oidc": providers })).into_response())
}

/// The login cookie's value, when exactly one cookie carries the name.
fn browser_secret<'h>(headers: &'h HeaderMap, cookie_name: &str) -> Option<&'h str> {
    let mut cookies = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|cookie| cookie.trim().split_once('='))
        .filter(|(name, _)| *name == cookie_name);
    let secret = cookies.next().map(|(_, value)| value)?;
    cookies
        .next()
        .is_none()
        .then_some(secret)
}

fn protect(response: Response) -> Response {
    let mut response = private_no_cache(response);
    response
        .headers_mut()
        .insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    response
        .headers_mut()
        .insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
        );
    response
        .headers_mut()
        .insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    response
}

pub(super) fn validate_workloads(config: &pnpr_config::Config) -> Result<(), RegistryError> {
    for workload in config.identity.auth.oidc.iter().flat_map(|provider| &provider.workloads) {
        if !matches!(
            config.routing.registries.get(&workload.registry),
            Some(pnpr_registry::Registry::Hosted { .. }),
        ) || config.routing.registries.ecosystem(&workload.registry)
            != Some(pnpr_registry::Ecosystem::Npm)
            || workload.packages.is_empty()
        {
            return Err(RegistryError::InvalidConfig {
                reason: "OIDC workloads require a hosted npm registry and explicit packages"
                    .to_string(),
            });
        }
        for package in &workload.packages {
            if !matches!(
                pnpr_registry::PackagePattern::parse(package, pnpr_registry::Ecosystem::Npm),
                Ok(pnpr_registry::PackagePattern::Exact(_)),
            ) {
                return Err(RegistryError::InvalidConfig {
                    reason: format!("invalid OIDC package {package:?}"),
                });
            }
        }
    }
    Ok(())
}

pub(super) fn check_workload_request(
    config: &pnpr_config::Config,
    workload: &pnpr_config::oidc::OidcWorkload,
    method: &axum::http::Method,
    path: &str,
) -> Result<(), RegistryError> {
    let decoded = pnpr_search::percent_decode(path);
    let base = config.routing.registries.base_path(pnpr_registry::Ecosystem::Npm);
    if *method == axum::http::Method::PUT
        && workload.packages
            .iter()
            .any(|package| decoded == format!("{base}/~{}/{package}", workload.registry))
    {
        return Ok(());
    }
    Err(RegistryError::Forbidden {
        user: workload.identity.username.clone(),
        action: "use",
        resource: "workload credentials outside the configured npm publish targets".to_string(),
    })
}

#[cfg(test)]
mod tests;
