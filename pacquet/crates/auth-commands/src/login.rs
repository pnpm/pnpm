//! `pnpm login` / `pnpm adduser` authenticates with an npm registry and
//! records the granted token in `auth.ini`.
//!
//! The command first tries the registry's web-based login (`POST -/v1/login`)
//! and, when the registry doesn't support it (HTTP 404 / 405), falls back to
//! classic username / password / email authentication (`PUT
//! -/user/org.couchdb.user:<name>`). Either path may raise a two-factor (OTP)
//! challenge, which is satisfied through [`pacquet_network_web_auth`] — a
//! browser round-trip when the registry offers web auth, or a prompted
//! one-time password otherwise.
//!
//! # Dependency-injection seam
//!
//! pnpm injects every side effect the flow touches as a bag of closures on a
//! `context` object. This port threads them through the project's capability
//! seam instead: the interactive OTP / web-auth effects reuse
//! [`pacquet_network_web_auth`]'s capability traits (composed on the single
//! `Sys` type parameter), the credential prompts are the crate-local
//! [`PromptInput`] / [`PromptPassword`] capabilities, and `auth.ini` I/O reuses
//! logout's [`FsReadToString`] / [`FsWrite`]. User-facing messages flow through
//! the `R: Reporter` seam on the `pnpm:global` channel, matching pnpm's
//! `globalInfo`. The two registry requests (the web-login `POST` and the
//! classic `PUT`) go over the shared [`ThrottledClient`] — a real fixture
//! (`mockito`) in tests — so only the effects a fixture can't stage portably
//! sit behind the `Sys` seam. See the "Dependency injection for tests" section
//! of `pacquet/CODE_STYLE_GUIDE.md`.

use std::{
    future::Future,
    io,
    path::{Path, PathBuf},
};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_network::{ThrottledClient, encode_uri_component, nerf_dart};
use pacquet_network_web_auth::{
    Clock, EnterKeyListener, GenerateQrCodeError, Host as WebAuthHost, OpenUrl, OtpChallenge,
    OtpError, PromptError, PromptOtp, Sleep, StdinIsTty, StdoutIsTty, SyntheticOtpError,
    WebAuthFetch, WebAuthFetchError, WebAuthFetchOptions, WebAuthFetchResponse,
    WebAuthRetryOptions, WebAuthTimeoutError, WebAuthTokenPollParams, WithOtpError,
    generate_qr_code, poll_for_web_auth_token, prompt_browser_open, with_otp_handling,
};
use pacquet_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use serde_json::{Value, json};

use crate::{
    ini::IniSettings,
    logout::{DEFAULT_REGISTRY, FsReadToString, FsWrite, normalize_registry_url},
};

/// Prompt for a line of visible input — the username and email prompts.
/// Mirrors pnpm's `enquirer.input({ message })`.
pub trait PromptInput {
    fn prompt_input(message: &str) -> impl Future<Output = Result<String, PromptError>>;
}

/// Prompt for a masked secret — the password prompt. Mirrors pnpm's
/// `enquirer.password({ message })`.
pub trait PromptPassword {
    fn prompt_password(message: &str) -> impl Future<Output = Result<String, PromptError>>;
}

/// Production provider for `pnpm login`. The credential prompts and `auth.ini`
/// I/O are real; every OTP / web-authentication capability delegates to
/// [`pacquet_network_web_auth::Host`], the shared production provider for that
/// flow.
pub struct Host;

impl FsReadToString for Host {
    fn read_to_string(path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }
}

impl FsWrite for Host {
    fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        pacquet_fs::write_atomic(path, bytes)
    }
}

impl PromptInput for Host {
    async fn prompt_input(message: &str) -> Result<String, PromptError> {
        prompt_line(message.to_owned(), Masking::Visible).await
    }
}

impl PromptPassword for Host {
    async fn prompt_password(message: &str) -> Result<String, PromptError> {
        prompt_line(message.to_owned(), Masking::Masked).await
    }
}

impl Clock for Host {
    fn now_ms() -> u64 {
        WebAuthHost::now_ms()
    }
}

impl Sleep for Host {
    fn sleep_ms(ms: u64) -> impl Future<Output = ()> {
        WebAuthHost::sleep_ms(ms)
    }
}

impl WebAuthFetch for Host {
    fn fetch(
        url: &str,
        options: &WebAuthFetchOptions,
    ) -> impl Future<Output = Result<WebAuthFetchResponse, WebAuthFetchError>> {
        WebAuthHost::fetch(url, options)
    }
}

impl StdinIsTty for Host {
    fn stdin_is_tty() -> bool {
        WebAuthHost::stdin_is_tty()
    }
}

impl StdoutIsTty for Host {
    fn stdout_is_tty() -> bool {
        WebAuthHost::stdout_is_tty()
    }
}

impl OpenUrl for Host {
    fn open_url(url: &str) -> io::Result<()> {
        WebAuthHost::open_url(url)
    }
}

impl EnterKeyListener for Host {
    type Handle = <WebAuthHost as EnterKeyListener>::Handle;
    fn listen() -> io::Result<Self::Handle> {
        WebAuthHost::listen()
    }
}

impl PromptOtp for Host {
    fn input(message: &str) -> impl Future<Output = Result<Option<String>, PromptError>> {
        WebAuthHost::input(message)
    }
}

/// Whether [`prompt_line`] echoes what the user types.
#[derive(Clone, Copy)]
enum Masking {
    Visible,
    Masked,
}

/// Read one line from the terminal with `dialoguer`, off the async runtime
/// (`dialoguer` is blocking). An interrupted prompt (Ctrl-C) maps to
/// [`PromptError::Cancelled`], mirroring enquirer's `ExitPromptError`.
async fn prompt_line(message: String, masking: Masking) -> Result<String, PromptError> {
    tokio::task::spawn_blocking(move || match masking {
        Masking::Visible => {
            dialoguer::Input::<String>::new().with_prompt(message).allow_empty(true).interact_text()
        }
        Masking::Masked => dialoguer::Password::new().with_prompt(message).interact(),
    })
    .await
    .map_err(|join_error| PromptError::Other { reason: join_error.to_string() })?
    .map_err(map_dialoguer_error)
}

fn map_dialoguer_error(error: dialoguer::Error) -> PromptError {
    match error {
        dialoguer::Error::IO(io) if io.kind() == io::ErrorKind::Interrupted => {
            PromptError::Cancelled
        }
        dialoguer::Error::IO(io) => PromptError::Other { reason: io.to_string() },
    }
}

/// Inputs to [`login`]. The retry / timeout knobs come from pnpm's
/// `fetchRetries` / `fetchTimeout` config and drive the web-auth poll.
pub struct LoginOptions<'a> {
    /// The `--registry` value; `None` falls back to [`DEFAULT_REGISTRY`].
    pub registry: Option<&'a str>,
    /// The `--scope` value; when set, the token is keyed to the scope and a
    /// scope-to-registry mapping is recorded.
    pub scope: Option<&'a str>,
    /// pnpm's `configDir`; `auth.ini` lives at `<config_dir>/auth.ini`.
    pub config_dir: &'a Path,
    pub fetch_retries: u32,
    pub fetch_retry_factor: u32,
    pub fetch_retry_mintimeout: u64,
    pub fetch_retry_maxtimeout: u64,
    pub fetch_timeout: u64,
}

/// Log in to `registry`, persist the granted token in `auth.ini`, and return
/// the `Logged in on <registry>` success line.
///
/// Tries web-based login first, falling back to classic
/// username / password / email login when the registry answers the web-login
/// probe with HTTP 404 or 405. Either path may satisfy a two-factor challenge
/// before returning.
pub async fn login<Sys, Reporter>(
    http_client: &ThrottledClient,
    opts: LoginOptions<'_>,
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
        + FsReadToString
        + FsWrite,
    Reporter: self::Reporter,
{
    let registry = normalize_registry_url(opts.registry.unwrap_or(DEFAULT_REGISTRY));

    if !Sys::stdin_is_tty() || !Sys::stdout_is_tty() {
        return Err(LoginError::NonInteractive);
    }

    let fetch_options = WebAuthFetchOptions {
        timeout: Some(opts.fetch_timeout),
        retry: Some(WebAuthRetryOptions {
            factor: Some(f64::from(opts.fetch_retry_factor)),
            max_timeout: Some(opts.fetch_retry_maxtimeout),
            min_timeout: Some(opts.fetch_retry_mintimeout),
            randomize: None,
            retries: Some(opts.fetch_retries),
        }),
    };

    let token = match web_login::<Sys, Reporter>(http_client, &registry, &fetch_options).await {
        Ok(token) => token,
        // Only a genuine web-login HTTP 404 / 405 means "web login unsupported";
        // every other failure (invalid response, poll timeout, transport) is
        // fatal and propagates.
        Err(WebLoginFlowError::Http { status, .. }) if status == 404 || status == 405 => {
            classic_login::<Sys, Reporter>(http_client, &registry, fetch_options).await?
        }
        Err(error) => return Err(error.into()),
    };

    let config_path = opts.config_dir.join("auth.ini");
    let mut settings = safe_read_ini::<Sys>(&config_path)?;
    let registry_config_key = nerf_dart(&registry);
    let scope_key = normalize_scope(opts.scope);
    let auth_config_key = match &scope_key {
        Some(scope) => format!("{registry_config_key}:{scope}"),
        None => registry_config_key,
    };
    settings.set(&format!("{auth_config_key}:_authToken"), &token);
    if let Some(scope) = &scope_key {
        settings.set(&format!("{scope}:registry"), &registry);
    }
    Sys::write(&config_path, settings.serialize().as_bytes())
        .map_err(|error| LoginError::WriteAuthIni { path: config_path.clone(), error })?;

    Ok(format!("Logged in on {registry}"))
}

/// Normalize a `--scope` value the way pnpm does: trim it, treat an empty
/// string or a bare `@` as "no scope", and prefix a missing leading `@`.
fn normalize_scope(scope: Option<&str>) -> Option<String> {
    let trimmed = scope?.trim();
    if trimmed.is_empty() || trimmed == "@" {
        return None;
    }
    Some(if trimmed.starts_with('@') { trimmed.to_owned() } else { format!("@{trimmed}") })
}

/// Read `auth.ini`, treating a missing file as empty. Any other read error
/// propagates.
fn safe_read_ini<Sys: FsReadToString>(path: &Path) -> Result<IniSettings, LoginError> {
    match Sys::read_to_string(path) {
        Ok(text) => Ok(IniSettings::parse(&text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(IniSettings::default()),
        Err(error) => Err(LoginError::ReadAuthIni { path: path.to_path_buf(), error }),
    }
}

/// Drive the registry's web-based login: probe `-/v1/login`, then poll the
/// returned `doneUrl` for the granted token while offering to open the login
/// URL in a browser.
async fn web_login<Sys, Reporter>(
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
        return Err(WebLoginFlowError::Http { status: response.status, text: response.body });
    }

    let json = serde_json::from_str::<Value>(&response.body).unwrap_or(Value::Null);
    let read = |field: &str| {
        json.get(field).and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_owned)
    };
    let (Some(auth_url), Some(done_url)) = (read("loginUrl"), read("doneUrl")) else {
        return Err(WebLoginFlowError::InvalidResponse);
    };

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

/// Prompt for username / password / email and register the user through
/// `PUT -/user/org.couchdb.user:<name>`, satisfying an OTP challenge if the
/// registry raises one.
async fn classic_login<Sys, Reporter>(
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
        + PromptPassword,
    Reporter: self::Reporter,
{
    let username = read_credential(Sys::prompt_input("Username:")).await?;
    let password = read_credential(Sys::prompt_password("Password:")).await?;
    let email = read_credential(Sys::prompt_input("Email (this IS public):")).await?;

    if username.is_empty() || password.is_empty() || email.is_empty() {
        return Err(LoginError::MissingCredentials);
    }

    let username_ref = username.as_str();
    let password_ref = password.as_str();
    let email_ref = email.as_str();
    let token = with_otp_handling::<Sys, Reporter, String, ClassicLoginOpError, _, _>(
        fetch_options,
        // A plain `FnMut` returning an `async move` block: the future is a
        // concrete type carrying only `Copy` borrows and the by-value OTP, so
        // it borrows nothing from the closure — see `with_otp_handling`'s
        // `Operation` bound.
        move |otp: Option<String>| async move {
            add_user(http_client, registry, username_ref, password_ref, email_ref, otp.as_deref())
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

/// Register a user via `PUT -/user/org.couchdb.user:<name>`, returning the
/// granted token. `otp` populates the `npm-otp` header on the retry pass.
async fn add_user(
    http_client: &ThrottledClient,
    registry: &str,
    username: &str,
    password: &str,
    email: &str,
    otp: Option<&str>,
) -> Result<String, AddUserError> {
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
    let www_authenticate = response
        .headers()
        .get(reqwest::header::WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let text = response.text().await.unwrap_or_default();

    if !ok {
        return Err(AddUserError::Http { status, text, www_authenticate });
    }

    match serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|json| json.get("token").and_then(Value::as_str).map(str::to_owned))
    {
        Some(token) if !token.is_empty() => Ok(token),
        _ => Err(AddUserError::NoToken),
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
        AddUserError::Http { status, text, .. } => ClassicLoginOpError::Failed { status, text },
        AddUserError::NoToken => ClassicLoginOpError::NoToken,
        AddUserError::Transport { reason } => ClassicLoginOpError::Transport { reason },
    }
}

/// Resolve `path` against the registry the way `new URL(path, registry)` does.
fn registry_join(registry: &str, path: &str) -> Result<String, url::ParseError> {
    url::Url::parse(registry)?.join(path).map(String::from)
}

fn global_info<Reporter: self::Reporter>(message: &str) {
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Info,
        message: message.to_owned(),
    }));
}

/// A materialized registry response — the fields [`web_login`] and
/// [`add_user`] read after the request completes.
struct HttpResponse {
    ok: bool,
    status: u16,
    body: String,
}

/// Failure surface of the web-based login flow. Internal to this module: the
/// caller either falls back to classic login (on [`Http`](Self::Http) 404/405)
/// or maps the rest into a [`LoginError`] via [`From`].
enum WebLoginFlowError {
    /// The web-login probe returned a non-success status.
    Http { status: u16, text: String },
    /// The probe succeeded but the body lacked a usable `loginUrl` / `doneUrl`.
    InvalidResponse,
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
            WebLoginFlowError::Timeout(timeout) => LoginError::WebAuthTimeout(timeout),
            WebLoginFlowError::QrCode(qr) => LoginError::QrCode(qr),
            WebLoginFlowError::Transport { reason } => LoginError::Request { reason },
        }
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
    #[diagnostic(code(ERR_PNPM_LOGIN_OTP_REQUIRED))]
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
    #[diagnostic(code(ERR_PNPM_LOGIN_REQUEST_FAILED))]
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

/// Errors surfaced by [`login`]. The user-facing variants carry pnpm's stable
/// error codes and messages verbatim; the classic-login and web-auth arms
/// surface their inner diagnostics transparently.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum LoginError {
    #[display("The login command requires an interactive terminal")]
    #[diagnostic(code(ERR_PNPM_LOGIN_NON_INTERACTIVE))]
    NonInteractive,

    #[display("The registry returned an invalid response for web-based login")]
    #[diagnostic(code(ERR_PNPM_LOGIN_INVALID_RESPONSE))]
    InvalidResponse,

    #[display("Username, password, and email are all required")]
    #[diagnostic(code(ERR_PNPM_LOGIN_MISSING_CREDENTIALS))]
    MissingCredentials,

    #[display("Login canceled")]
    #[diagnostic(code(ERR_PNPM_LOGIN_CANCELED))]
    Canceled,

    #[display("Web-based login failed (HTTP {status}): {text}")]
    #[diagnostic(code(ERR_PNPM_WEB_LOGIN_FAILED))]
    WebLoginFailed { status: u16, text: String },

    #[display("{_0}")]
    #[diagnostic(transparent)]
    ClassicLogin(WithOtpError<ClassicLoginOpError>),

    #[display("{_0}")]
    #[diagnostic(transparent)]
    WebAuthTimeout(WebAuthTimeoutError),

    #[display("Failed to render the login QR code: {_0}")]
    #[diagnostic(code(ERR_PNPM_LOGIN_QR_CODE))]
    QrCode(#[error(source)] GenerateQrCodeError),

    #[display("The login request failed: {reason}")]
    #[diagnostic(code(ERR_PNPM_LOGIN_REQUEST_FAILED))]
    Request {
        #[error(not(source))]
        reason: String,
    },

    #[display("Failed to read the login prompt: {reason}")]
    #[diagnostic(code(ERR_PNPM_LOGIN_PROMPT_FAILED))]
    Prompt {
        #[error(not(source))]
        reason: String,
    },

    #[display("Failed to read auth.ini at {}: {error}", path.display())]
    #[diagnostic(code(pacquet_auth_commands::read_auth_ini))]
    ReadAuthIni {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to write auth.ini at {}: {error}", path.display())]
    #[diagnostic(code(pacquet_auth_commands::write_auth_ini))]
    WriteAuthIni {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

#[cfg(test)]
mod tests;
