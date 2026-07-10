use std::future::Future;

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_network::{ThrottledClient, encode_uri_component, redact_and_sanitize};
use pacquet_network_web_auth::{
    Clock, EnterKeyListener, OpenUrl, OtpChallenge, OtpError, PromptError, PromptOtp, Sleep,
    StdinIsTty, StdoutIsTty, SyntheticOtpError, WebAuthFetch, WebAuthFetchOptions,
    with_otp_handling,
};
use pacquet_reporter::Reporter;
use serde_json::{Value, json};

use super::{
    error::LoginError,
    global_info,
    prompt::{Masking, PromptInput, PromptPassword, prompt_line},
    registry_join,
};

/// Prompt for username / password / email and register the user through
/// `PUT -/user/org.couchdb.user:<name>`, satisfying an OTP challenge if the
/// registry raises one.
pub(super) async fn classic_login<Sys, Reporter>(
    http_client: &ThrottledClient,
    registry: &str,
    fetch_options: WebAuthFetchOptions,
) -> Result<String, LoginError>
where
    Sys: Clock
        + Sleep
        + WebAuthFetch
        + StdinIsTty
        + StdoutIsTty
        + EnterKeyListener
        + OpenUrl
        + PromptOtp
        + PromptInput
        + PromptPassword
        + 'static,
    Reporter: self::Reporter,
{
    let username = read_credential(prompt_line::<Sys>("Username:", Masking::Visible)).await?;
    let password = read_credential(prompt_line::<Sys>("Password:", Masking::Masked)).await?;
    let email =
        read_credential(prompt_line::<Sys>("Email (this IS public):", Masking::Visible)).await?;

    if username.is_empty() || password.is_empty() || email.is_empty() {
        return Err(LoginError::MissingCredentials);
    }

    let credentials = Credentials { username: &username, password: &password, email: &email };
    let token = with_otp_handling::<Sys, Reporter, String, ClassicLoginOpError, _, _>(
        fetch_options,
        // A plain `FnMut` returning an `async move` block: the future is a
        // concrete type carrying only `Copy` borrows and the by-value OTP, so
        // it borrows nothing from the closure — see `with_otp_handling`'s
        // `Operation` bound.
        move |otp: Option<String>| async move {
            add_user(http_client, registry, credentials, otp.as_deref())
                .await
                .map_err(add_user_error_to_op::<Reporter>)
        },
    )
    .await
    .map_err(LoginError::ClassicLogin)?;

    global_info::<Reporter>(&format!("Logged in as {username}"));

    Ok(token)
}

/// Await a credential prompt, mapping an aborted prompt to
/// [`LoginError::Canceled`] and any other failure to [`LoginError::Prompt`].
async fn read_credential(
    prompt: impl Future<Output = Result<String, PromptError>>,
) -> Result<String, LoginError> {
    match prompt.await {
        Ok(value) => Ok(value),
        Err(PromptError::Cancelled) => Err(LoginError::Canceled),
        Err(error) => Err(LoginError::Prompt { reason: error.to_string() }),
    }
}

/// The user-supplied login credentials that [`add_user`] submits. Bundling the
/// three same-typed `&str` values behind named fields keeps them from being
/// transposed at the call site.
#[derive(Clone, Copy)]
struct Credentials<'a> {
    username: &'a str,
    password: &'a str,
    email: &'a str,
}

/// Register a user via `PUT -/user/org.couchdb.user:<name>`, returning the
/// granted token. `otp` populates the `npm-otp` header on the retry pass.
async fn add_user(
    http_client: &ThrottledClient,
    registry: &str,
    credentials: Credentials<'_>,
    otp: Option<&str>,
) -> Result<String, AddUserError> {
    let Credentials { username, password, email } = credentials;
    let url = registry_join(
        registry,
        &format!("-/user/org.couchdb.user:{}", encode_uri_component(username)),
    )
    .map_err(|error| AddUserError::Transport { reason: error.to_string() })?;
    let document = json!({
        "_id": format!("org.couchdb.user:{username}"),
        "name": username,
        "password": password,
        "email": email,
        "type": "user",
    });
    let body = serde_json::to_string(&document).expect("serialize addUser document");

    let guard = http_client.acquire_for_url(&url).await;
    let mut request = guard
        .put(&url)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .header("npm-auth-type", "web")
        .body(body);
    if let Some(otp) = otp {
        request = request.header("npm-otp", otp);
    }

    let response = request
        .send()
        .await
        .map_err(|error| AddUserError::Transport { reason: error.to_string() })?;
    let ok = response.status().is_success();
    let status = response.status().as_u16();
    // Join every `WWW-Authenticate` header the way the Fetch `Headers.get`
    // pnpm relies on does, so an `otp` challenge that isn't the first of
    // several challenge headers is still detected.
    let www_authenticate = {
        let joined = response
            .headers()
            .get_all(reqwest::header::WWW_AUTHENTICATE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect::<Vec<_>>()
            .join(", ");
        (!joined.is_empty()).then_some(joined)
    };
    let text = response.text().await.unwrap_or_default();

    if !ok {
        return Err(AddUserError::Http { status, text, www_authenticate });
    }

    match serde_json::from_str::<Value>(&text) {
        Ok(parsed) => match parsed.get("token").and_then(Value::as_str) {
            Some(token) if !token.is_empty() => Ok(token.to_owned()),
            _ => Err(AddUserError::NoToken),
        },
        Err(_) => Err(AddUserError::NoToken),
    }
}

/// Classify an [`add_user`] failure for [`with_otp_handling`]: a `401` that
/// advertises `otp` in `WWW-Authenticate` becomes an OTP challenge (its body
/// parsed for `authUrl` / `doneUrl`), everything else a terminal error.
fn add_user_error_to_op<Reporter: self::Reporter>(error: AddUserError) -> ClassicLoginOpError {
    match error {
        // Mirrors pnpm's `err.responseHeaders.get('www-authenticate')?.includes('otp')`.
        AddUserError::Http { status, text, www_authenticate }
            if status == 401
                && www_authenticate.as_deref().is_some_and(|value| value.contains("otp")) =>
        {
            let json = serde_json::from_str::<Value>(&text).ok();
            let challenge = SyntheticOtpError::from_unknown_body::<Reporter>(json.as_ref())
                .as_otp_challenge()
                .expect("SyntheticOtpError is always an OTP challenge");
            ClassicLoginOpError::Otp { challenge }
        }
        AddUserError::Http { status, text, .. } => {
            ClassicLoginOpError::Failed { status, text: redact_and_sanitize(&text) }
        }
        AddUserError::NoToken => ClassicLoginOpError::NoToken,
        AddUserError::Transport { reason } => ClassicLoginOpError::Transport { reason },
    }
}

/// Failure surface of [`add_user`]. Internal to this module: mapped into a
/// [`ClassicLoginOpError`] for [`with_otp_handling`].
enum AddUserError {
    /// The registry returned a non-success status.
    Http { status: u16, text: String, www_authenticate: Option<String> },
    /// The registry accepted the request but returned no token.
    NoToken,
    /// The request never produced a response.
    Transport { reason: String },
}

/// The error the classic-login operation hands to [`with_otp_handling`]. The
/// [`Otp`](Self::Otp) arm is the only challenge it acts on; the rest are
/// terminal and surface (transparently) as the login result's error, carrying
/// pnpm's stable codes.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ClassicLoginOpError {
    #[display("The registry requires a one-time password to complete the login")]
    #[diagnostic(code(pacquet_auth_commands::login_otp_required))]
    Otp {
        #[error(not(source))]
        challenge: OtpChallenge,
    },

    #[display("Login failed (HTTP {status}): {text}")]
    #[diagnostic(code(ERR_PNPM_LOGIN_FAILED))]
    Failed { status: u16, text: String },

    #[display("The registry did not return an authentication token")]
    #[diagnostic(code(ERR_PNPM_LOGIN_NO_TOKEN))]
    NoToken,

    #[display("The login request failed: {reason}")]
    #[diagnostic(code(pacquet_auth_commands::login_request_failed))]
    Transport {
        #[error(not(source))]
        reason: String,
    },
}

impl OtpError for ClassicLoginOpError {
    fn as_otp_challenge(&self) -> Option<OtpChallenge> {
        match self {
            ClassicLoginOpError::Otp { challenge } => Some(challenge.clone()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
