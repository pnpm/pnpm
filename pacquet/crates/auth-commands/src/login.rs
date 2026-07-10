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
//! `Sys` type parameter), the credential prompts read through the crate-local
//! [`PromptInput`] / [`PromptPassword`] capabilities — the raw `dialoguer`
//! terminal reads, wrapped by `prompt_line` — and `auth.ini` I/O reuses
//! logout's [`FsReadToString`] / [`FsWrite`]. User-facing messages flow through
//! the `Reporter` seam on the `pnpm:global` channel, matching pnpm's
//! `globalInfo`. The two registry requests (the web-login `POST` and the
//! classic `PUT`) go over the shared [`ThrottledClient`] — a real fixture
//! (`mockito`) in tests — so only the effects a fixture can't stage portably
//! sit behind the `Sys` seam. See the "Dependency injection for tests" section
//! of `pacquet/CODE_STYLE_GUIDE.md`.

use std::{io, path::Path};

use pacquet_network::{ThrottledClient, nerf_dart, redact_and_sanitize};
use pacquet_network_web_auth::{
    Clock, EnterKeyListener, OpenUrl, PromptOtp, Sleep, StdinIsTty, StdoutIsTty, WebAuthFetch,
    WebAuthFetchOptions, WebAuthRetryOptions,
};
use pacquet_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};

use crate::{
    ini::IniSettings,
    logout::{DEFAULT_REGISTRY, FsReadToString, FsWrite},
    registry_url::normalize_registry_url,
};

mod classic_login;
mod error;
mod host;
mod prompt;
mod web_login;

pub use classic_login::ClassicLoginOpError;
pub use error::LoginError;
pub use host::Host;
pub use prompt::{PromptInput, PromptPassword};

use classic_login::classic_login;
use web_login::{WebLoginFlowError, web_login};

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

/// The full capability set [`login`] requires from its host: the eight
/// OTP / web-auth effects, the two credential prompts ([`PromptInput`] /
/// [`PromptPassword`]), and `auth.ini` read / write ([`FsReadToString`] /
/// [`FsWrite`]). The blanket impl covers every type that implements all of
/// them, so the production [`Host`] and the test fakes satisfy it
/// automatically. Bundling the bound lets a caller that re-dispatches into
/// [`login`] — the CLI adapter — name one trait instead of restating the list.
pub trait LoginHost:
    Clock
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
    + FsWrite
    + 'static
{
}

impl<Sys> LoginHost for Sys where
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
        + FsWrite
        + 'static
{
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
    Sys: LoginHost,
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
        .map_err(move |error| LoginError::WriteAuthIni { path: config_path, error })?;

    // A registry from an untrusted `.npmrc` / `--registry` can embed
    // `user:pass@` credentials or terminal escape sequences, so redact and
    // sanitize before it reaches stdout. Matches `pnpm logout` / `ping`.
    Ok(format!("Logged in on {}", redact_and_sanitize(&registry)))
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

#[cfg(test)]
mod test_classic_login;
#[cfg(test)]
mod test_non_interactive;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod test_web_login;
#[cfg(test)]
mod test_web_login_errors;
#[cfg(test)]
mod test_web_login_scope;
