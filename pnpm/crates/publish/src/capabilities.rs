//! Per-capability dependency-injection traits and the production [`Host`]
//! provider for the publish flow.
//!
//! Each side effect the publish flow needs — HTTP requests, the clock,
//! environment reads, subprocess spawns — is its own
//! `self`-less capability trait, composed as bounds on a single `Sys` type
//! parameter, with the real OS behind [`Host`] and `fn`-bound unit-struct
//! fakes in tests. See the "Dependency injection for tests" section of
//! `pnpm/CODE_STYLE_GUIDE.md`.
//!
//! The OTP / web-authentication side effects (the clock, the sleep timer, the
//! browser opener, the "press Enter" listener, the classic-OTP prompt) are
//! *not* re-declared here: they already have a seam in
//! [`pnpm_network_web_auth`], whose [`pnpm_network_web_auth::Host`] this
//! crate reuses to drive the publish request's OTP handling.

use std::{
    sync::LazyLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// The subprocess capability (used to run the configured token helper and
/// the git working-tree checks) and the production provider every
/// capability trait below is also implemented for.
///
/// Both are defined in the `pnpm-git-utils` crate — the git queries need
/// the same seam — and re-exported here so this crate's callers keep
/// importing them from `pnpm_publish` alongside the rest.
pub use pnpm_git_utils::{CommandOutput, Host, RunCommand};

/// Read an environment variable.
///
/// Returns `None` when the variable is unset or holds invalid UTF-8. An empty
/// value is returned as `Some("")`; call sites that treat empty as unset
/// filter it themselves.
pub trait EnvVar {
    fn var(name: &str) -> Option<String>;
}

/// Read the current wall-clock time as Unix-epoch milliseconds. Used only to
/// log how long the GitHub id-token request took.
pub trait Clock {
    fn now_ms() -> u64;
}

/// HTTP method used by the OIDC requests. They are always a bodyless `GET` or
/// a `POST` with an empty body, so the method is the only request shape that
/// varies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OidcMethod {
    Get,
    Post,
}

/// One OIDC request: the id-token fetch, the token exchange, or the
/// package-visibility probe.
#[derive(Debug, Clone)]
pub struct OidcRequest<'a> {
    pub method: OidcMethod,
    pub url: &'a str,
    /// Full `Authorization` header value, e.g. `Bearer <token>`.
    pub authorization: &'a str,
    /// Per-request timeout in milliseconds.
    pub timeout_ms: Option<u64>,
}

/// A materialized OIDC response. The body is handed back as raw text so the
/// caller parses it (and classifies a malformed body) itself.
#[derive(Debug, Clone)]
pub struct OidcResponse {
    pub ok: bool,
    pub status: u16,
    pub body: String,
}

/// The OIDC request never produced a response — the transport itself failed.
/// The auth-token path catches this and surfaces it as a fetch failure.
#[derive(Debug, derive_more::Display, derive_more::Error)]
#[display("the OIDC request failed: {reason}")]
pub struct OidcFetchError {
    pub reason: String,
}

/// Perform a single OIDC request.
pub trait OidcFetch {
    fn fetch(
        request: OidcRequest<'_>,
    ) -> impl Future<Output = Result<OidcResponse, OidcFetchError>>;
}

/// Ask the user a yes/no question, used when the current branch is not the
/// publish branch.
pub trait ConfirmPrompt {
    /// Return the user's answer; an aborted prompt (Ctrl-C) is `false`.
    fn confirm(message: &str) -> bool;
}

impl EnvVar for Host {
    fn var(name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

impl Clock for Host {
    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
    }
}

impl OidcFetch for Host {
    async fn fetch(request: OidcRequest<'_>) -> Result<OidcResponse, OidcFetchError> {
        // One process-wide client so repeated OIDC calls reuse connections.
        static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

        let mut builder = match request.method {
            OidcMethod::Get => CLIENT.get(request.url),
            OidcMethod::Post => CLIENT.post(request.url).header("content-length", "0").body(""),
        };
        builder = builder
            .header("accept", "application/json")
            .header("authorization", request.authorization);
        if let Some(timeout) = request.timeout_ms {
            builder = builder.timeout(Duration::from_millis(timeout));
        }
        let response =
            builder.send().await.map_err(|error| OidcFetchError { reason: error.to_string() })?;
        let ok = response.status().is_success();
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        Ok(OidcResponse { ok, status, body })
    }
}

impl ConfirmPrompt for Host {
    fn confirm(message: &str) -> bool {
        dialoguer::Confirm::new().with_prompt(message).interact().unwrap_or(false)
    }
}

#[cfg(test)]
mod tests;
