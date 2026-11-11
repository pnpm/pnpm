use axum::{
    Json,
    extract::{Extension, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use pnpr_auth::oidc::{LoginReturn, OidcState, WebLoginPoll};
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
/// the CLI printed.
pub(crate) async fn get_v1_login_device(
    State(state): State<AppState>,
    TargetRegistry(_): TargetRegistry,
) -> Response {
    if login_providers(&state).is_empty() {
        return not_found();
    }
    code_page(StatusCode::OK, None)
}

/// `POST /-/v1/login/device`: the form body holds the `code` the user typed
/// and, once chosen, the `provider` to sign in with. With one OIDC login
/// provider, or with `provider` set, claims the web login and starts the
/// provider sign-in that completes it. With several providers and no
/// `provider`, checks the code and asks for the provider.
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
    if let Err(refusal) = admit_device_request(&state, &headers, client_ip, peer_addr) {
        return refusal;
    }
    let form = DeviceForm::parse(&body);
    let code = form.code.unwrap_or_default();
    let oidc = &state.inner.identity.oidc;
    let provider = match (form.provider, providers.as_slice()) {
        (None, [only]) => only.name.clone(),
        (None, _) if oidc.is_pending_web_login(&code) => return provider_page(&providers, &code),
        (None, _) => return code_page(StatusCode::BAD_REQUEST, Some(UNUSABLE_CODE)),
        (Some(provider), _)
            if providers
                .iter()
                .any(|known| known.name == provider) =>
        {
            provider
        }
        (Some(_), _) => return bad_request("unknown sign-in provider"),
    };
    sign_in_with_code(oidc, &provider, &code).await
}

/// Refuse a code submission that did not come from pnpr's own page, or that
/// exceeds the web login rate limit.
fn admit_device_request(
    state: &AppState,
    headers: &HeaderMap,
    client_ip: ClientIpExtension,
    peer_addr: PeerAddrExtension,
) -> Result<(), Response> {
    if !is_same_origin_request(headers) {
        return Err(bad_request(
            "submit the code from the sign-in page, opened over HTTPS or on localhost",
        ));
    }
    let (ip_address, peer_ip) = resolve_rate_limit_ips(client_ip, peer_addr);
    state.inner.identity.login_rate_limiter
        .check_and_record_web_login(ip_address, peer_ip)
        .map_err(|retry_after| super::user_accounts::login_throttled_response(state, retry_after))
}

/// Claim the web login that `code` belongs to and send the browser to
/// `provider`. A sign-in that cannot start releases the claim.
async fn sign_in_with_code(oidc: &OidcState, provider: &str, code: &str) -> Response {
    let Some(login_id) = oidc.claim_web_login(code) else {
        return code_page(StatusCode::BAD_REQUEST, Some(UNUSABLE_CODE));
    };
    let returns_to = LoginReturn::Cli { login_id: login_id.clone() };
    redirect_to_provider(oidc, provider, returns_to).await
        .unwrap_or_else(|err| {
            oidc.release_web_login(&login_id);
            protect(err.into_response())
        })
}

const UNUSABLE_CODE: &str =
    "That code is unknown, expired, or already used. Run the login command again.";

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

fn html_page(status: StatusCode, body: &str) -> Response {
    let html = format!(
        r#"<!doctype html>
<html>
<head><title>Sign In - pnpr</title><meta charset="utf-8"></head>
<body>
  <h1>Command-line sign-in</h1>
{body}
</body>
</html>"#,
    );
    protect((status, [(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response())
}

fn code_page(status: StatusCode, error: Option<&str>) -> Response {
    let error = error
        .map(|error| format!("  <p><strong>{}</strong></p>\n", escape_html(error)))
        .unwrap_or_default();
    html_page(
        status,
        &format!(
            r#"  <p>Enter the code that your terminal shows. If you did not start a sign-in from a terminal, close this page.</p>
{error}  <form method="post">
    <p><label>Code <input name="code" required autofocus autocomplete="off" autocapitalize="characters" spellcheck="false"></label></p>
    <p><button type="submit">Continue</button></p>
  </form>"#,
        ),
    )
}

/// The second page, for a pnpr with several OIDC login providers: the code
/// was accepted, and the user picks where to sign in. Only the browser that
/// typed the code receives it back here.
fn provider_page(providers: &[&pnpr_config::oidc::OidcProvider], code: &str) -> Response {
    let mut buttons = String::new();
    for provider in providers {
        let name = escape_html(&provider.name);
        let _ = write!(
            buttons,
            r#"
    <p><button type="submit" name="provider" value="{name}">Sign in with {name}</button></p>"#,
        );
    }
    let code = escape_html(code);
    html_page(
        StatusCode::OK,
        &format!(
            r#"  <p>Choose where you sign in.</p>
  <form method="post">
    <input type="hidden" name="code" value="{code}">{buttons}
  </form>"#,
        ),
    )
}

fn escape_html(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
