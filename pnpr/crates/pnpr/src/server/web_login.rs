use axum::{
    Json,
    extract::{Extension, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use pnpr_auth::oidc::{LoginReturn, WebLoginPoll};
use serde::Deserialize;
use serde_json::json;

use std::{fmt::Write as _, net::IpAddr};

use super::{
    AppState, TargetRegistry, not_found,
    oidc::{is_valid_state, protect, redirect_to_provider},
    private_no_cache,
};

#[cfg(test)]
mod tests;

#[derive(Deserialize)]
pub(crate) struct PollPath {
    session: String,
}

type ClientIpExtension = Option<Extension<super::ClientIp>>;
type PeerAddrExtension = Option<Extension<axum::extract::ConnectInfo<super::PeerAddr>>>;

fn resolve_rate_limit_ips(
    client_ip: ClientIpExtension,
    peer_addr: PeerAddrExtension,
) -> (Option<IpAddr>, Option<IpAddr>) {
    let ip = client_ip.map(|Extension(super::ClientIp(addr))| addr);
    let peer =
        peer_addr.map(|Extension(axum::extract::ConnectInfo(super::PeerAddr(addr)))| addr.ip());
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
        format!("{base}{prefix}/-/v1/login/device"),
        format!("{base}{prefix}/-/v1/login/poll/{}", tokens.poll_token),
    )
}

/// `POST /-/v1/login` or `POST /~{registry}/-/v1/login`: initiate CLI web
/// login. The `npm-notice` header carries the user code to type on the
/// `loginUrl` page.
pub(crate) async fn post_v1_login(
    State(state): State<AppState>,
    TargetRegistry(registry): TargetRegistry,
    client_ip: ClientIpExtension,
    peer_addr: PeerAddrExtension,
) -> Response {
    if login_providers(&state).is_empty() {
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
    let notice = format!("Enter the code {} in the browser to sign in", tokens.user_code);
    let Ok(notice) = HeaderValue::from_str(&notice) else {
        return pnpr_error::RegistryError::Internal {
            reason: "invalid web login user code".to_string(),
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

/// `GET /-/v1/login/device`: the browser page where the user types the code
/// the CLI printed and picks an OIDC provider.
pub(crate) async fn get_v1_login_device(
    State(state): State<AppState>,
    TargetRegistry(_): TargetRegistry,
) -> Response {
    let providers = login_providers(&state);
    if providers.is_empty() {
        return not_found();
    }
    device_page(StatusCode::OK, &providers, None)
}

/// `POST /-/v1/login/device`: claims the web login whose code the form
/// carries and starts the provider sign-in that completes it. The form body
/// holds `code` and `provider`.
pub(crate) async fn post_v1_login_device(
    State(state): State<AppState>,
    TargetRegistry(_): TargetRegistry,
    client_ip: ClientIpExtension,
    peer_addr: PeerAddrExtension,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let providers = login_providers(&state);
    if providers.is_empty() {
        return not_found();
    }
    if !is_same_origin_request(&headers) {
        return bad_request("submit the code from the sign-in page");
    }
    let (ip_address, peer_ip) = resolve_rate_limit_ips(client_ip, peer_addr);
    if let Err(retry_after) =
        state.inner.identity.login_rate_limiter.check_and_record_web_login(ip_address, peer_ip)
    {
        return super::user_accounts::login_throttled_response(&state, retry_after);
    }
    let form = DeviceForm::parse(&body);
    let Some(provider) = providers
        .iter()
        .find(|provider| Some(provider.name.as_str()) == form.provider.as_deref())
    else {
        return bad_request("unknown sign-in provider");
    };
    let Some(login_id) = state.inner.identity.oidc.claim_web_login(&form.code.unwrap_or_default())
    else {
        return device_page(
            StatusCode::BAD_REQUEST,
            &providers,
            Some("That code is unknown, expired, or already used. Run the login command again."),
        );
    };
    redirect_to_provider(&state.inner.identity.oidc, &provider.name, LoginReturn::Cli { login_id })
        .await
}

#[derive(Default)]
struct DeviceForm {
    code: Option<String>,
    provider: Option<String>,
}

impl DeviceForm {
    fn parse(body: &[u8]) -> Self {
        let mut form = DeviceForm::default();
        for (key, value) in url::form_urlencoded::parse(body) {
            match key.as_ref() {
                "code" => form.code = Some(value.into_owned()),
                "provider" => form.provider = Some(value.into_owned()),
                _ => {}
            }
        }
        form
    }
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

fn device_page(
    status: StatusCode,
    providers: &[&pnpr_config::oidc::OidcProvider],
    error: Option<&str>,
) -> Response {
    let html = render_device_page(providers, error);
    protect((status, [(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response())
}

fn render_device_page(
    providers: &[&pnpr_config::oidc::OidcProvider],
    error: Option<&str>,
) -> String {
    let mut buttons = String::new();
    for provider in providers {
        let name = escape_html(&provider.name);
        let _ = write!(
            buttons,
            r#"<p><button type="submit" name="provider" value="{name}">Sign in with {name}</button></p>"#,
        );
    }
    let error = error
        .map(|error| format!("<p><strong>{}</strong></p>", escape_html(error)))
        .unwrap_or_default();
    format!(
        r#"<!doctype html>
<html>
<head><title>Sign In - pnpr</title><meta charset="utf-8"></head>
<body>
  <h1>Command-line sign-in</h1>
  <p>Enter the code that your terminal shows. If you did not start a sign-in from a terminal, close this page.</p>
  {error}
  <form method="post">
    <p><label>Code <input name="code" required autofocus autocomplete="off" autocapitalize="characters" spellcheck="false"></label></p>
    {buttons}
  </form>
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
