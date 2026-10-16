use axum::{
    Json,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use pnpr_auth::oidc::WebLoginPoll;
use serde::Deserialize;
use serde_json::json;

use std::fmt::Write as _;

use super::{
    AppState, TargetRegistry, not_found,
    oidc::{is_valid_state, protect},
    private_no_cache,
};

#[derive(Deserialize)]
pub(crate) struct StartQuery {
    session: String,
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
    private_no_cache(
        (
            StatusCode::ACCEPTED,
            Json(json!({
                "loginUrl": login_url,
                "doneUrl": done_url,
            })),
        )
            .into_response(),
    )
}

/// `GET /-/v1/login/start?session={session}`: browser entry point for web login.
pub(crate) async fn get_v1_login_start(
    State(state): State<AppState>,
    TargetRegistry(_): TargetRegistry,
    Query(query): Query<StartQuery>,
) -> Response {
    if !is_valid_state(&query.session)
        || !state.inner.identity.oidc.is_valid_web_login(&query.session)
    {
        return bad_request("invalid or expired login session");
    }
    let providers: Vec<_> = state.inner.config.identity.auth.oidc
        .iter()
        .filter(|provider| provider.login.is_some())
        .collect();
    if providers.is_empty() {
        return not_found();
    }
    let public_url = state.inner.config.http.public_url.trim_end_matches('/');
    if providers.len() == 1 {
        let redirect_url = format!(
            "{public_url}/-/oidc/{}/login?return=cli&session={}",
            providers[0].name, query.session,
        );
        return protect(Redirect::to(&redirect_url).into_response());
    }
    let html = render_provider_picker(&providers, &query.session, public_url);
    protect(
        (StatusCode::OK, [(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response(
        ),
    )
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

fn render_provider_picker(
    providers: &[&pnpr_config::oidc::OidcProvider],
    session: &str,
    public_url: &str,
) -> String {
    let mut list = String::new();
    for provider in providers {
        let name = escape_html(&provider.name);
        let _ = write!(
            list,
            r#"<li><a href="{public_url}/-/oidc/{name}/login?return=cli&amp;session={session}">Sign in with {name}</a></li>"#,
        );
    }
    format!(
        r#"<!doctype html>
<html>
<head><title>Sign In - pnpr</title><meta charset="utf-8"></head>
<body>
  <h1>Select an identity provider</h1>
  <ul>{list}</ul>
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
