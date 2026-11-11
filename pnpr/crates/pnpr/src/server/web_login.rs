use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use pnpr_auth::oidc::{LoginReturn, WebLoginPoll};
use serde::Deserialize;
use serde_json::json;

use std::fmt::Write as _;

use super::{
    AppState, TargetRegistry, not_found,
    oidc::{is_valid_state, protect, redirect_to_provider},
    private_no_cache,
};

#[cfg(test)]
mod tests;

#[derive(Deserialize)]
pub(crate) struct StartQuery {
    session: String,
}

#[derive(Deserialize)]
pub(crate) struct ConfirmQuery {
    session: String,
    provider: String,
}

#[derive(Deserialize)]
pub(crate) struct PollPath {
    session: String,
}

fn has_oidc_login_provider(state: &AppState) -> bool {
    state.inner.config.identity.auth.oidc.iter().any(|provider| provider.login.is_some())
}

fn resolve_rate_limit_ips(
    client_ip: Option<axum::extract::Extension<super::ClientIp>>,
    peer_addr: Option<axum::extract::Extension<axum::extract::ConnectInfo<super::PeerAddr>>>,
) -> (Option<std::net::IpAddr>, Option<std::net::IpAddr>) {
    let ip = client_ip.map(|axum::extract::Extension(super::ClientIp(addr))| addr);
    let peer = peer_addr.map(
        |axum::extract::Extension(axum::extract::ConnectInfo(super::PeerAddr(addr)))| addr.ip(),
    );
    (ip, peer)
}

fn build_login_urls(
    public_url: &str,
    registry: Option<&str>,
    tokens: &pnpr_auth::oidc::WebLoginTokens,
) -> (String, String) {
    let base = public_url.trim_end_matches('/');
    let prefix = registry
        .map(|reg| format!("/~{reg}"))
        .unwrap_or_default();
    (
        format!("{base}{prefix}/-/v1/login/start?session={}", tokens.browser_session_id),
        format!("{base}{prefix}/-/v1/login/poll/{}", tokens.poll_token),
    )
}

/// `POST /-/v1/login` or `POST /~{registry}/-/v1/login`: initiate CLI web login.
pub(crate) async fn post_v1_login(
    State(state): State<AppState>,
    TargetRegistry(registry): TargetRegistry,
    client_ip: Option<axum::extract::Extension<super::ClientIp>>,
    peer_addr: Option<axum::extract::Extension<axum::extract::ConnectInfo<super::PeerAddr>>>,
) -> Response {
    if !has_oidc_login_provider(&state) {
        return not_found();
    }
    let (ip_address, peer_ip) = resolve_rate_limit_ips(client_ip, peer_addr);
    if let Err(retry_after) =
        state.inner.identity.login_rate_limiter.check_and_record_web_login(ip_address, peer_ip)
    {
        return super::user_accounts::login_throttled_response(&state, retry_after);
    }
    let tokens = match state.inner.identity.oidc.create_web_login() {
        Ok(tokens) => tokens,
        Err(err) => return err.into_response(),
    };
    let (login_url, done_url) =
        build_login_urls(&state.inner.config.http.public_url, registry.as_deref(), &tokens);
    let notice = format!("Sign in only if the browser shows the code {}", tokens.confirmation_code);
    let Ok(notice) = HeaderValue::from_str(&notice) else {
        return pnpr_error::RegistryError::Internal {
            reason: "invalid web login confirmation code".to_string(),
        }
        .into_response();
    };
    private_no_cache(
        (
            StatusCode::ACCEPTED,
            [("npm-notice", notice)],
            Json(json!({
                "loginUrl": login_url,
                "doneUrl": done_url,
            })),
        )
            .into_response(),
    )
}

/// `GET /-/v1/login/start?session={session}`: browser entry point for web
/// login. Shows the session's confirmation code, which the CLI also prints,
/// and a button per OIDC provider that confirms the sign-in.
pub(crate) async fn get_v1_login_start(
    State(state): State<AppState>,
    TargetRegistry(_): TargetRegistry,
    Query(query): Query<StartQuery>,
) -> Response {
    let Some(code) = pending_confirmation_code(&state, &query.session) else {
        return bad_request("invalid or expired login session");
    };
    let providers = login_providers(&state);
    if providers.is_empty() {
        return not_found();
    }
    let html = render_confirmation_page(&providers, &query.session, &code);
    protect(
        (StatusCode::OK, [(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response(
        ),
    )
}

/// `POST /-/v1/login/start?session={session}&provider={provider}`: the user
/// confirmed the sign-in on the page [`get_v1_login_start`] served. Starts
/// the provider sign-in that completes the CLI session.
pub(crate) async fn post_v1_login_start(
    State(state): State<AppState>,
    TargetRegistry(_): TargetRegistry,
    Query(query): Query<ConfirmQuery>,
    headers: HeaderMap,
) -> Response {
    if !is_same_origin_request(&headers) {
        return bad_request("confirm the sign-in on the page that shows its code");
    }
    if pending_confirmation_code(&state, &query.session).is_none() {
        return bad_request("invalid or expired login session");
    }
    if !login_providers(&state)
        .iter()
        .any(|provider| provider.name == query.provider)
    {
        return bad_request("unknown sign-in provider");
    }
    redirect_to_provider(
        &state.inner.identity.oidc,
        &query.provider,
        LoginReturn::Cli { session: query.session },
    )
    .await
}

fn pending_confirmation_code(state: &AppState, session: &str) -> Option<String> {
    if !is_valid_state(session) {
        return None;
    }
    state.inner.identity.oidc.pending_web_login_code(session)
}

fn login_providers(state: &AppState) -> Vec<&pnpr_config::oidc::OidcProvider> {
    state.inner.config.identity.auth.oidc
        .iter()
        .filter(|provider| provider.login.is_some())
        .collect()
}

/// Whether a browser sent the request from one of pnpr's own pages. Another
/// site can submit a form to pnpr, but cannot make the browser mark it
/// same-origin. The page's `Referrer-Policy: no-referrer` makes browsers send
/// `Origin: null`, so `Origin` cannot stand in for this header.
fn is_same_origin_request(headers: &HeaderMap) -> bool {
    headers
        .get("sec-fetch-site")
        .is_some_and(|site| site == "same-origin")
}

/// `GET /-/v1/login/poll/{session}`: poll for granted CLI token.
pub(crate) async fn get_v1_login_poll(
    State(state): State<AppState>,
    TargetRegistry(_): TargetRegistry,
    Path(path): Path<PollPath>,
) -> Response {
    if !is_valid_state(&path.session) {
        return not_found();
    }
    match state.inner.identity.oidc.poll_web_login(&path.session) {
        WebLoginPoll::NotFound => not_found(),
        WebLoginPoll::Pending => private_no_cache(
            (StatusCode::ACCEPTED, [(header::RETRY_AFTER, "1")], Json(json!({}))).into_response(),
        ),
        WebLoginPoll::Ready(token) => {
            private_no_cache((StatusCode::OK, Json(json!({ "token": token }))).into_response())
        }
    }
}

fn bad_request(reason: &str) -> Response {
    protect(pnpr_error::RegistryError::BadRequest { reason: reason.to_string() }.into_response())
}

fn render_confirmation_page(
    providers: &[&pnpr_config::oidc::OidcProvider],
    session: &str,
    code: &str,
) -> String {
    let mut buttons = String::new();
    for provider in providers {
        let action = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("session", session)
            .append_pair("provider", &provider.name)
            .finish();
        let _ = write!(
            buttons,
            r#"<form method="post" action="?{}"><button type="submit">Sign in with {}</button></form>"#,
            escape_html(&action),
            escape_html(&provider.name),
        );
    }
    let code = escape_html(code);
    format!(
        r#"<!doctype html>
<html>
<head><title>Confirm Sign In - pnpr</title><meta charset="utf-8"></head>
<body>
  <h1>Confirm command-line sign-in</h1>
  <p>A command-line tool asked to sign in to this registry. Continue only if you started the sign-in yourself.</p>
  <p>If your terminal shows a code, it must be <strong>{code}</strong>.</p>
  {buttons}
</body>
</html>"#,
    )
}

fn escape_html(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
