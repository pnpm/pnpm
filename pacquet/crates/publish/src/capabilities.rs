//! Per-capability dependency-injection traits and the production [`Host`]
//! provider for the publish flow.
//!
//! The TypeScript `publish` command injects every side effect — `fetch`,
//! `Date.now`, the `ci-info` probes, `process.env`, `execa` — as closures on
//! a `context`/`process` bag (see `utils/shared-context.ts`). This crate
//! ports that seam to pacquet's convention: one `self`-less capability trait
//! per effect, composed as bounds on a single `Sys` type parameter, with the
//! real OS behind [`Host`] and `fn`-bound unit-struct fakes in tests. See the
//! "Dependency injection for tests" section of `pacquet/CODE_STYLE_GUIDE.md`.
//!
//! The OTP / web-authentication side effects (the clock, the sleep timer, the
//! browser opener, the "press Enter" listener, the classic-OTP prompt) are
//! *not* re-declared here: they already have a seam in
//! [`pacquet_network_web_auth`], whose [`pacquet_network_web_auth::Host`] this
//! crate reuses for [`crate::publish_with_otp_handling`].

use std::{
    io,
    path::Path,
    process::Command,
    sync::LazyLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Read an environment variable. Mirrors a `process.env.<NAME>` read.
///
/// Returns `None` when the variable is unset or holds invalid UTF-8. An empty
/// value is returned as `Some("")`; call sites that mirror JavaScript
/// truthiness (`if (env.X)`) filter it themselves.
pub trait EnvVar {
    fn var(name: &str) -> Option<String>;
}

/// Detect the continuous-integration provider. Mirrors the two `ci-info`
/// fields the publish command reads (`GITHUB_ACTIONS`, `GITLAB`).
pub trait CiInfo {
    fn github_actions() -> bool;
    fn gitlab() -> bool;
}

/// Read the current wall-clock time as Unix-epoch milliseconds. Mirrors TS
/// `Date.now()`; used only to log how long the GitHub id-token request took.
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
/// package-visibility probe. Ports the field-for-field options the TS
/// `fetch` closures receive.
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
/// caller parses (and classifies a malformed body) exactly as the TS code
/// does with `response.json()`.
#[derive(Debug, Clone)]
pub struct OidcResponse {
    pub ok: bool,
    pub status: u16,
    pub body: String,
}

/// `fetch` itself failed — the request never produced a response. Mirrors the
/// TS `fetch(...)` promise rejecting (caught and wrapped as
/// `AuthTokenFetchError` by the auth-token path).
#[derive(Debug, derive_more::Display, derive_more::Error)]
#[display("the OIDC request failed: {reason}")]
pub struct OidcFetchError {
    pub reason: String,
}

/// Perform a single OIDC request. Mirrors the injected `fetch(url, options)`.
pub trait OidcFetch {
    fn fetch(
        request: OidcRequest<'_>,
    ) -> impl Future<Output = Result<OidcResponse, OidcFetchError>>;
}

/// Captured output of a spawned subprocess. Mirrors the `stdout` / `stderr` /
/// exit-status fields the publish command reads from `execa`.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Run a subprocess and capture its output. Mirrors TS `execa(cmd, args)`;
/// used by the `tokenHelper` execution and the git working-tree checks.
pub trait RunCommand {
    fn run(program: &str, args: &[&str], cwd: Option<&Path>) -> io::Result<CommandOutput>;
}

/// Ask the user a yes/no question. Mirrors the publish command's
/// `@inquirer/prompts` `confirm`, used when the current branch is not the
/// publish branch.
pub trait ConfirmPrompt {
    /// Return the user's answer; an aborted prompt (Ctrl-C) is `false`,
    /// matching the TS `ExitPromptError` handling.
    fn confirm(message: &str) -> bool;
}

/// Production implementation of every capability trait in this crate. Each
/// method calls into the real OS facility (`std::env`, `SystemTime`,
/// `reqwest`, `std::process::Command`).
pub struct Host;

impl EnvVar for Host {
    fn var(name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

impl CiInfo for Host {
    fn github_actions() -> bool {
        env_is_truthy("GITHUB_ACTIONS")
    }

    fn gitlab() -> bool {
        env_is_truthy("GITLAB_CI")
    }
}

/// Mirror `ci-info`'s `!!process.env.<NAME>`: set and non-empty is `true`.
fn env_is_truthy(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| !value.is_empty())
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

impl RunCommand for Host {
    fn run(program: &str, args: &[&str], cwd: Option<&Path>) -> io::Result<CommandOutput> {
        let mut command = Command::new(program);
        command.args(args);
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let output = command.output()?;
        Ok(CommandOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

impl ConfirmPrompt for Host {
    fn confirm(message: &str) -> bool {
        dialoguer::Confirm::new().with_prompt(message).interact().unwrap_or(false)
    }
}
