use super::{AppState, private_no_cache};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use pnpr_auth::oidc::{HANDOFF_TTL, LoginReturn, LoginSession, OidcState};
use pnpr_error::RegistryError;
use serde::Deserialize;
use serde_json::json;

/// The web UI's page that redeems a handoff code, relative to
/// `/-/oidc/{provider}/callback`.
const UI_HANDOFF_PAGE: &str = "../../ui/sign-in/oidc";

/// Carries a handoff code from the callback to the web UI's redemption
/// request. Only the browser that signed in holds it, so a link cannot sign
/// another browser in. The name ends with the sign-in's OIDC state, its
/// `flow`, so parallel sign-ins in one browser keep their own codes.
const HANDOFF_COOKIE_PREFIX: &str = "__Host-pnpr-oidc-handoff-";

/// The browser sign-in routes. A sign-in can end in the web UI only when
/// `ui_served`.
pub(super) fn routes(ui_served: bool) -> Router<AppState> {
    Router::new()
        .route(
            "/-/oidc/{provider}/login",
            get(move |state: State<AppState>, provider: Path<String>, query: Query<LoginQuery>| {
                login(state, provider, query, ui_served)
            }),
        )
        .route("/-/oidc/{provider}/callback", get(callback))
        .route("/-/oidc/handoff", post(redeem_handoff))
        .route("/-/pnpr/v0/sign-in", get(sign_in_methods))
}

#[derive(Deserialize)]
pub(super) struct LoginQuery {
    #[serde(rename = "return")]
    returns_to: Option<String>,
}

async fn login(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(query): Query<LoginQuery>,
    ui_served: bool,
) -> Response {
    let returns_to = match query.returns_to.as_deref() {
        None => LoginReturn::Token,
        Some("ui") if ui_served => LoginReturn::Ui,
        Some("ui") => return bad_request("this pnpr does not serve the web UI"),
        Some(_) => return bad_request("return must be ui"),
    };
    let response = match state.inner.identity.oidc.start(&provider, returns_to).await {
        Ok(start) => with_cookie(
            Redirect::to(&start.url).into_response(),
            &format!(
                "__Host-pnpr-oidc-{}={}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=300",
                start.state, start.browser_secret,
            ),
        ),
        Err(err) => err.into_response(),
    };
    protect(response)
}

#[derive(Deserialize)]
pub(super) struct Callback {
    state: String,
    code: Option<String>,
}

async fn callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(query): Query<Callback>,
    headers: HeaderMap,
) -> Response {
    if !is_valid_state(&query.state) {
        return bad_request("invalid OIDC state");
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
            Ok(session) => signed_in(&state.inner.identity.oidc, &query.state, session),
            Err(err) => err.into_response(),
        }
    } else {
        RegistryError::Unauthenticated { resource: "OIDC browser session".to_string() }
            .into_response()
    };
    with_cookie(protect(response), &expired_cookie(&cookie_name))
}

/// Where the browser lands after `session`, the sign-in `flow`, signed in:
/// the web UI, holding a handoff code in a [`HANDOFF_COOKIE_PREFIX`] cookie,
/// or a page that shows the token.
fn signed_in(oidc: &OidcState, flow: &str, session: LoginSession) -> Response {
    if session.returns_to == LoginReturn::Ui {
        return match oidc.hand_off(session) {
            Ok(code) => with_cookie(
                Redirect::to(&format!("{UI_HANDOFF_PAGE}?flow={flow}")).into_response(),
                &format!(
                    "{HANDOFF_COOKIE_PREFIX}{flow}={code}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age={}",
                    HANDOFF_TTL.as_secs(),
                ),
            ),
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
struct HandoffQuery {
    flow: String,
}

/// `POST /-/oidc/handoff?flow=<flow>`: trades the handoff code that the
/// sign-in `flow`, started with `return=ui`, left in a
/// [`HANDOFF_COOKIE_PREFIX`] cookie for the session token.
async fn redeem_handoff(
    State(state): State<AppState>,
    Query(query): Query<HandoffQuery>,
    headers: HeaderMap,
) -> Response {
    if !is_valid_state(&query.flow) {
        return bad_request("invalid sign-in flow");
    }
    redeem(&state.inner.identity.oidc, &query.flow, &headers)
}

fn redeem(oidc: &OidcState, flow: &str, headers: &HeaderMap) -> Response {
    let cookie_name = format!("{HANDOFF_COOKIE_PREFIX}{flow}");
    let response =
        match browser_secret(headers, &cookie_name).map(|code| oidc.redeem_handoff(code)) {
            Some(Ok(session)) => {
                Json(json!({ "token": session.token, "expires": session.expires })).into_response()
            }
            Some(Err(err)) => err.into_response(),
            None => RegistryError::Unauthenticated { resource: "OIDC handoff".to_string() }
                .into_response(),
        };
    with_cookie(protect(response), &expired_cookie(&cookie_name))
}

/// An OIDC state as pnpr issues it, safe to put in a cookie name.
fn is_valid_state(state: &str) -> bool {
    !state.is_empty()
        && state.len() <= 128
        && state
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// `GET /-/pnpr/v0/sign-in`: the OIDC providers that offer browser sign-in.
async fn sign_in_methods(State(state): State<AppState>) -> Response {
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

fn expired_cookie(name: &str) -> String {
    format!("{name}=; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=0")
}

fn with_cookie(mut response: Response, cookie: &str) -> Response {
    match HeaderValue::from_str(cookie) {
        Ok(cookie) => {
            response.headers_mut().append(header::SET_COOKIE, cookie);
            response
        }
        Err(_) => {
            RegistryError::Internal { reason: "invalid OIDC cookie".to_string() }.into_response()
        }
    }
}

fn bad_request(reason: &str) -> Response {
    protect(RegistryError::BadRequest { reason: reason.to_string() }.into_response())
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
