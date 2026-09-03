//! `pnpm login` / `pnpm adduser` authenticates with an npm registry and
//! records the granted token in the global `config.yaml`.
//!
//! The command first tries the registry's web-based login (`POST -/v1/login`)
//! and, when the registry doesn't support it (HTTP 404 / 405), falls back to
//! classic username / password / email authentication (`PUT
//! -/user/org.couchdb.user:<name>`). Either path may raise a two-factor (OTP)
//! challenge, which is satisfied through [`pnpm_network_web_auth`] — a
//! browser round-trip when the registry offers web auth, or a prompted
//! one-time password otherwise.
//!
//! # Dependency-injection seam
//!
//! pnpm injects every side effect the flow touches as a bag of closures on a
//! `context` object. This port threads them through the project's capability
//! seam instead: the interactive OTP / web-auth effects reuse
//! [`pnpm_network_web_auth`]'s capability traits (composed on the single
//! `Sys` type parameter), the credential prompts read through the crate-local
//! [`PromptInput`] / [`PromptPassword`] capabilities — the raw `dialoguer`
//! terminal reads, wrapped by `prompt_line` — and `config.yaml` I/O reuses
//! logout's [`FsReadToString`] / [`FsWrite`]. User-facing messages flow through
//! the `Reporter` seam on the `pnpm:global` channel, matching pnpm's
//! `globalInfo`. The two registry requests (the web-login `POST` and the
//! classic `PUT`) go over the shared [`ThrottledClient`] — a real fixture
//! (`mockito`) in tests — so only the effects a fixture can't stage portably
//! sit behind the `Sys` seam. See the "Dependency injection for tests" section
//! of `pnpm/CODE_STYLE_GUIDE.md`.

use std::{io, path::Path};

use pnpm_config::{is_json_auth_scope, validate_json_auth_registry};
use pnpm_network::{ThrottledClient, redact_and_sanitize};
use pnpm_network_web_auth::{
    Clock, EnterKeyListener, OpenUrl, PromptOtp, Sleep, StdinIsTty, StdoutIsTty, WebAuthFetch,
    WebAuthFetchOptions, WebAuthRetryOptions,
};
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pnpm_workspace_manifest_writer::{ManifestEdit, edit_manifest_field};

use crate::{
    config_yaml::{self, GLOBAL_CONFIG_YAML_FILENAME},
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
    /// The scope to key the token to; a scope-to-registry mapping is
    /// recorded alongside it. `None` records an unscoped token.
    pub scope: Option<&'a str>,
    /// pnpm's `configDir`; the global config lives at
    /// `<config_dir>/config.yaml`.
    pub config_dir: &'a Path,
    pub fetch_retries: u32,
    pub fetch_retry_factor: u32,
    pub fetch_retry_mintimeout: u64,
    pub fetch_retry_maxtimeout: u64,
    pub fetch_timeout: u64,
}

/// The full capability set [`login`] requires from its host: the eight
/// OTP / web-auth effects, the two credential prompts ([`PromptInput`] /
/// [`PromptPassword`]), and `config.yaml` read / write ([`FsReadToString`] /
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

/// Log in to `registry`, persist the granted token in the global
/// `config.yaml`, and return the `Logged in on <registry>` success line.
///
/// Tries web-based login first, falling back to classic
/// username / password / email login when the registry answers the web-login
/// probe with HTTP 404 or 405. Either path may satisfy a two-factor challenge
/// before returning.
///
/// The web-based flow runs without an interactive terminal — it prints the
/// authentication URL and polls the registry until the browser approval
/// completes. Only the classic flow's credential prompts require a TTY.
pub async fn login<Sys, Reporter>(
    http_client: &ThrottledClient,
    opts: LoginOptions<'_>,
) -> Result<String, LoginError>
where
    Sys: LoginHost,
    Reporter: self::Reporter,
{
    let registry = normalize_registry_url(opts.registry.unwrap_or(DEFAULT_REGISTRY));
    // Before the network, not after: a value the reader would refuse must not
    // cost the user a round-trip, and must never be written — a `config.yaml`
    // holding one fails to load for every later command.
    let registry = validate_json_auth_registry(&registry)
        .map_err(|reason| LoginError::UnrecordableLogin { reason })?;
    if let Some(scope) = normalize_scope(opts.scope)
        && !is_json_auth_scope(&scope)
    {
        return Err(LoginError::UnrecordableLogin {
            reason: format!(r#"the scope {scope:?} is not a package scope like "@org""#),
        });
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

    record_login::<Sys>(&opts, &registry, &token)?;

    // A registry from an untrusted `.npmrc` / `--registry` can embed
    // `user:pass@` credentials or terminal escape sequences, so redact and
    // sanitize before it reaches stdout. Matches `pnpm logout` / `ping`.
    Ok(format!("Logged in on {}", redact_and_sanitize(&registry)))
}

/// Normalize a scope value the way pnpm does: trim it, treat an empty
/// string or a bare `@` as "no scope", and prefix a missing leading `@`.
fn normalize_scope(scope: Option<&str>) -> Option<String> {
    let trimmed = scope?.trim();
    if trimmed.is_empty() || trimmed == "@" {
        return None;
    }
    Some(if trimmed.starts_with('@') { trimmed.to_owned() } else { format!("@{trimmed}") })
}

/// Record the granted `token` in the global `config.yaml`: the credential
/// under `_auth`, and the route to it under `registries`.
///
/// The fields are folded into the document one after another and written
/// once. A credential and the route that reaches it are one fact, so a
/// failure part-way through has to leave the file as it was rather than a
/// token the command is about to report it failed to record.
fn record_login<Sys: FsReadToString + FsWrite>(
    opts: &LoginOptions<'_>,
    registry: &str,
    token: &str,
) -> Result<(), LoginError> {
    let config_path = opts.config_dir.join(GLOBAL_CONFIG_YAML_FILENAME);
    let original = read_config_yaml::<Sys>(&config_path)?;
    let scope = normalize_scope(opts.scope);
    let fields = config_yaml::login_fields(original.as_deref(), registry, scope.as_deref(), token)
        .map_err(LoginError::ParseConfigYaml)?;

    let mut document = original.clone();
    for (key, value) in fields {
        match edit_manifest_field(document.as_deref(), key, &value)
            .map_err(LoginError::EditConfigYaml)?
        {
            ManifestEdit::Write(text) => document = Some(text),
            ManifestEdit::Unchanged => {}
            // Only a deletion empties a document, and a login sets.
            ManifestEdit::Remove => {
                unreachable!("recording a login cannot empty {}", config_path.display())
            }
        }
    }

    let Some(text) = document.filter(|text| Some(text) != original.as_ref()) else {
        return Ok(());
    };
    Sys::write(&config_path, text.as_bytes())
        .map_err(|error| LoginError::WriteConfigYaml { path: config_path, error })
}

/// Read the global `config.yaml`, treating a missing file as absent. Any
/// other read error propagates.
fn read_config_yaml<Sys: FsReadToString>(path: &Path) -> Result<Option<String>, LoginError> {
    match Sys::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(LoginError::ReadConfigYaml { path: path.to_path_buf(), error }),
    }
}

/// Resolve `path` against the registry the way `new URL(path, registry)` does.
fn registry_join(registry: &str, path: &str) -> Result<String, url::ParseError> {
    url::Url::parse(registry)?.join(path).map(String::from)
}

fn global_info<Reporter: self::Reporter>(message: String) {
    Reporter::emit(&LogEvent::Global(GlobalLog { level: LogLevel::Info, message }));
}

#[cfg(test)]
mod support;
#[cfg(test)]
mod test_classic_login;
#[cfg(test)]
mod test_non_interactive;
#[cfg(test)]
mod test_web_login;
#[cfg(test)]
mod test_web_login_errors;
#[cfg(test)]
mod test_web_login_scope;
