use pacquet_diagnostics::miette::{self, Diagnostic};
use pacquet_reporter::Reporter;
use serde_json::Value;

use crate::{
    GenerateQrCodeError, WebAuthTimeoutError,
    capabilities::{
        Clock, EnterKeyListener, OpenUrl, PromptError, PromptOtp, Sleep, StdinIsTty, StdoutIsTty,
        WebAuthFetch,
    },
    generate_qr_code::generate_qr_code,
    global_log::{global_info, global_warn},
    poll_for_web_auth_token::{
        WebAuthFetchOptions, WebAuthTokenPollParams, poll_for_web_auth_token,
    },
    prompt_browser_open::prompt_browser_open,
};

#[cfg(test)]
mod tests;

/// The `authUrl` / `doneUrl` an OTP challenge may carry. Both are optional
/// because a registry may send neither (a classic OTP) or a malformed
/// body. Ports TS `OtpErrorBody`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OtpErrorBody {
    pub auth_url: Option<String>,
    pub done_url: Option<String>,
}

/// An EOTP challenge surfaced by an operation's error. Ports the
/// `{ code: 'EOTP', body? }` shape pnpm detects with `isOtpError`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OtpChallenge {
    pub body: Option<OtpErrorBody>,
}

/// Implemented by an operation's error type so [`with_otp_handling`] can
/// detect an EOTP challenge and read its body. Ports TS `isOtpError` plus
/// the `error.body` read: returning `Some` is the moral equivalent of
/// `error.code === 'EOTP'`.
pub trait OtpError {
    fn as_otp_challenge(&self) -> Option<OtpChallenge>;
}

/// Synthetic EOTP error meant to be thrown by an operation passed to
/// [`with_otp_handling`] and caught by it — never to propagate elsewhere.
/// Ports TS `SyntheticOtpError`.
#[derive(Debug, derive_more::Display, derive_more::Error, Clone)]
#[display(
    "This error was meant to be caught by `with_otp_handling`, not to propagate to other parts of \
     the code"
)]
pub struct SyntheticOtpError {
    body: Option<OtpErrorBody>,
}

impl SyntheticOtpError {
    #[must_use]
    pub fn new(body: Option<OtpErrorBody>) -> Self {
        SyntheticOtpError { body }
    }

    /// Build a challenge from an arbitrary JSON body, keeping only string
    /// `authUrl` / `doneUrl` fields and warning (via the `R: Reporter`
    /// global-warn seam) when either is present with a non-string type.
    /// Ports TS `SyntheticOtpError.fromUnknownBody`.
    #[must_use]
    pub fn from_unknown_body<Reporter: self::Reporter>(body: Option<&Value>) -> Self {
        let Some(Value::Object(map)) = body else {
            return SyntheticOtpError { body: None };
        };
        let auth_url = extract_url_field::<Reporter>(map, "authUrl");
        let done_url = extract_url_field::<Reporter>(map, "doneUrl");
        SyntheticOtpError { body: Some(OtpErrorBody { auth_url, done_url }) }
    }
}

impl OtpError for SyntheticOtpError {
    fn as_otp_challenge(&self) -> Option<OtpChallenge> {
        Some(OtpChallenge { body: self.body.clone() })
    }
}

fn extract_url_field<Reporter: self::Reporter>(
    map: &serde_json::Map<String, Value>,
    field: &str,
) -> Option<String> {
    match map.get(field) {
        None => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(other) => {
            global_warn::<Reporter>(&format!(
                "OTP error body: {field} has type {}, expected string",
                js_typeof(other),
            ));
            None
        }
    }
}

/// JavaScript's `typeof` for a JSON value, used to mirror pnpm's warning
/// text. Note `typeof null === 'object'` and `typeof [] === 'object'`.
fn js_typeof(value: &Value) -> &'static str {
    match value {
        Value::Null | Value::Array(_) | Value::Object(_) => "object",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
    }
}

/// The registry required additional authentication but the terminal is not
/// interactive. Ports pnpm's `OtpNonInteractiveError`
/// (`ERR_PNPM_OTP_NON_INTERACTIVE`).
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
#[display(
    "The registry requires additional authentication, but pnpm is not running in an interactive \
     terminal"
)]
#[diagnostic(
    code(ERR_PNPM_OTP_NON_INTERACTIVE),
    help(
        "Re-run this command in an interactive terminal to complete authentication, or provide the \
         --otp option if you are using a classic one-time password (OTP)"
    )
)]
pub struct OtpNonInteractiveError;

/// The registry asked for an OTP a second time after one was already
/// supplied. Ports pnpm's `OtpSecondChallengeError`
/// (`ERR_PNPM_OTP_SECOND_CHALLENGE`).
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
#[display(
    "The registry requested a one-time password (OTP) a second time after one was already provided"
)]
#[diagnostic(
    code(ERR_PNPM_OTP_SECOND_CHALLENGE),
    help(
        "This is unexpected behavior from the registry. Try the command again later and, if the \
         issue persists, verify that your registry supports OTP-based authentication or contact \
         the registry administrator."
    )
)]
pub struct OtpSecondChallengeError;

/// Failure surface of [`with_otp_handling`]. `Operation` carries the
/// caller's own error (the original challenge re-thrown, or a non-OTP
/// failure from either operation attempt); the rest are this crate's
/// errors.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum WithOtpError<Error: Diagnostic + 'static> {
    #[display("{_0}")]
    #[diagnostic(transparent)]
    Operation(Error),

    #[display("{_0}")]
    #[diagnostic(transparent)]
    NonInteractive(OtpNonInteractiveError),

    #[display("{_0}")]
    #[diagnostic(transparent)]
    SecondChallenge(OtpSecondChallengeError),

    #[display("{_0}")]
    #[diagnostic(transparent)]
    Timeout(WebAuthTimeoutError),

    #[display("{_0}")]
    Prompt(PromptError),

    #[display("{_0}")]
    QrCode(GenerateQrCodeError),
}

/// Run `operation`, transparently satisfying an OTP challenge if it raises
/// one.
///
/// On the first [`OtpError`] the operation returns, this either drives the
/// web-based authentication flow (when the challenge body carries both
/// `authUrl` and `doneUrl`) or prompts for a classic OTP, then retries the
/// operation once with the obtained one-time password. Any non-OTP error,
/// or an OTP challenge with no usable code, propagates unchanged.
///
/// Ports pnpm's `withOtpHandling`.
pub async fn with_otp_handling<Sys, Reporter, Token, Error, Operation>(
    fetch_options: WebAuthFetchOptions,
    mut operation: Operation,
) -> Result<Token, WithOtpError<Error>>
where
    Sys: Clock
        + Sleep
        + WebAuthFetch
        + StdinIsTty
        + StdoutIsTty
        + EnterKeyListener
        + OpenUrl
        + PromptOtp,
    Reporter: self::Reporter,
    Error: OtpError + Diagnostic + 'static,
    Operation: AsyncFnMut(Option<&str>) -> Result<Token, Error>,
{
    let error = match operation(None).await {
        Ok(value) => return Ok(value),
        Err(error) => error,
    };

    let Some(challenge) = error.as_otp_challenge() else {
        return Err(WithOtpError::Operation(error));
    };

    if !Sys::stdin_is_tty() || !Sys::stdout_is_tty() {
        return Err(WithOtpError::NonInteractive(OtpNonInteractiveError));
    }

    let otp = match challenge.body {
        Some(OtpErrorBody { auth_url: Some(auth_url), done_url: Some(done_url) }) => {
            let qr_code = generate_qr_code(&auth_url).map_err(WithOtpError::QrCode)?;
            global_info::<Reporter>(&format!(
                "Authenticate your account at:\n{auth_url}\n\n{qr_code}",
            ));
            let poll = poll_for_web_auth_token::<Sys>(WebAuthTokenPollParams {
                done_url,
                fetch_options,
                timeout_ms: None,
            });
            prompt_browser_open::<Sys, Reporter, _, _>(&auth_url, poll)
                .await
                .map(Some)
                .map_err(WithOtpError::Timeout)?
        }
        _ => match Sys::input("This operation requires a one-time password.\nEnter OTP:").await {
            Ok(value) => value.filter(|otp| !otp.is_empty()),
            // The user aborted the prompt: re-throw the original challenge,
            // matching pnpm's `ExitPromptError` handling.
            Err(PromptError::Cancelled) => return Err(WithOtpError::Operation(error)),
            Err(other) => return Err(WithOtpError::Prompt(other)),
        },
    };

    let Some(otp) = otp else {
        return Err(WithOtpError::Operation(error));
    };

    match operation(Some(&otp)).await {
        Ok(value) => Ok(value),
        Err(retry_error) if retry_error.as_otp_challenge().is_some() => {
            Err(WithOtpError::SecondChallenge(OtpSecondChallengeError))
        }
        Err(retry_error) => Err(WithOtpError::Operation(retry_error)),
    }
}
