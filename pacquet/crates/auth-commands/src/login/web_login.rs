use pacquet_network::{ThrottledClient, redact_and_sanitize};
use pacquet_network_web_auth::{
    Clock, EnterKeyListener, GenerateQrCodeError, OpenUrl, Sleep, StdinIsTty, WebAuthFetch,
    WebAuthFetchOptions, WebAuthTimeoutError, WebAuthTokenPollParams, generate_qr_code,
    poll_for_web_auth_token, prompt_browser_open,
};
use pacquet_reporter::Reporter;
use serde_json::Value;

use super::{error::LoginError, global_info, registry_join};

/// Drive the registry's web-based login: probe `-/v1/login`, then poll the
/// returned `doneUrl` for the granted token while offering to open the login
/// URL in a browser.
pub(super) async fn web_login<Sys, Reporter>(
    http_client: &ThrottledClient,
    registry: &str,
    fetch_options: &WebAuthFetchOptions,
) -> Result<String, WebLoginFlowError>
where
    Sys: Clock + Sleep + WebAuthFetch + StdinIsTty + EnterKeyListener + OpenUrl,
    Reporter: self::Reporter,
{
    let login_url = registry_join(registry, "-/v1/login")
        .map_err(|error| WebLoginFlowError::Transport { reason: error.to_string() })?;
    let response = web_login_post(http_client, &login_url).await?;
    if !response.ok {
        let text = redact_and_sanitize(&response.body);
        return Err(WebLoginFlowError::Http { status: response.status, text });
    }

    let json = serde_json::from_str::<Value>(&response.body).unwrap_or(Value::Null);
    let read = |field: &str| {
        json.get(field).and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_owned)
    };
    let (Some(auth_url), Some(done_url)) = (read("loginUrl"), read("doneUrl")) else {
        return Err(WebLoginFlowError::InvalidResponse);
    };

    // A legitimate login / done URL is a plain URL; a control character (a
    // terminal escape, CR, or LF) is never valid in one and signals a malicious
    // or compromised registry trying to spoof the terminal. Reject the login so
    // the user learns the registry misbehaved, rather than sanitizing the URL and
    // authenticating against it anyway.
    if auth_url.contains(char::is_control) || done_url.contains(char::is_control) {
        return Err(WebLoginFlowError::UnsafeUrl);
    }

    let qr_code = generate_qr_code(&auth_url).map_err(WebLoginFlowError::QrCode)?;
    global_info::<Reporter>(&format!("Authenticate your account at:\n{auth_url}\n\n{qr_code}"));

    let poll = poll_for_web_auth_token::<Sys>(WebAuthTokenPollParams {
        done_url,
        fetch_options: fetch_options.clone(),
        timeout_ms: None,
    });
    prompt_browser_open::<Sys, Reporter, WebAuthTimeoutError, _>(&auth_url, poll)
        .await
        .map_err(WebLoginFlowError::Timeout)
}

/// Send the `POST -/v1/login` web-login probe and materialize its response.
async fn web_login_post(
    http_client: &ThrottledClient,
    login_url: &str,
) -> Result<HttpResponse, WebLoginFlowError> {
    let guard = http_client.acquire_for_url(login_url).await;
    let response = guard
        .post(login_url)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .header("npm-auth-type", "web")
        .body("{}")
        .send()
        .await
        .map_err(|error| WebLoginFlowError::Transport { reason: error.to_string() })?;
    let ok = response.status().is_success();
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    Ok(HttpResponse { ok, status, body })
}

/// A materialized registry response — the fields [`web_login`] and
/// `add_user` read after the request completes.
struct HttpResponse {
    ok: bool,
    status: u16,
    body: String,
}

/// Failure surface of the web-based login flow. Internal to this module: the
/// caller either falls back to classic login (on [`Http`](Self::Http) 404/405)
/// or maps the rest into a [`LoginError`] via [`From`].
pub(super) enum WebLoginFlowError {
    /// The web-login probe returned a non-success status.
    Http { status: u16, text: String },
    /// The probe succeeded but the body lacked a usable `loginUrl` / `doneUrl`.
    InvalidResponse,
    /// A `loginUrl` / `doneUrl` carried control characters — a terminal-spoofing
    /// attempt — so the login is rejected rather than used.
    UnsafeUrl,
    /// The token poll exceeded its budget.
    Timeout(WebAuthTimeoutError),
    /// The login URL could not be rendered as a QR code.
    QrCode(GenerateQrCodeError),
    /// The probe request never produced a response.
    Transport { reason: String },
}

impl From<WebLoginFlowError> for LoginError {
    fn from(error: WebLoginFlowError) -> Self {
        match error {
            WebLoginFlowError::Http { status, text } => LoginError::WebLoginFailed { status, text },
            WebLoginFlowError::InvalidResponse => LoginError::InvalidResponse,
            WebLoginFlowError::UnsafeUrl => LoginError::UnsafeLoginUrl,
            WebLoginFlowError::Timeout(timeout) => LoginError::WebAuthTimeout(timeout),
            WebLoginFlowError::QrCode(qr) => LoginError::QrCode(qr),
            WebLoginFlowError::Transport { reason } => LoginError::Request { reason },
        }
    }
}
