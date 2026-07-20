use std::{io, path::PathBuf};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_network_web_auth::{GenerateQrCodeError, WebAuthTimeoutError, WithOtpError};

use super::classic_login::ClassicLoginOpError;

/// Errors surfaced by [`super::login`]. The user-facing variants carry pnpm's
/// stable error codes and messages verbatim; the classic-login and web-auth
/// arms surface their inner diagnostics transparently.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum LoginError {
    #[display("The login command requires an interactive terminal")]
    #[diagnostic(code(ERR_PNPM_LOGIN_NON_INTERACTIVE))]
    NonInteractive,

    #[display("The registry returned an invalid response for web-based login")]
    #[diagnostic(code(ERR_PNPM_LOGIN_INVALID_RESPONSE))]
    InvalidResponse,

    #[display(
        "The registry returned a login URL containing control characters and was rejected as a \
         possible terminal-spoofing attempt"
    )]
    #[diagnostic(code(ERR_PNPM_AUTH_COMMANDS_LOGIN_UNSAFE_URL))]
    UnsafeLoginUrl,

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
    #[diagnostic(code(ERR_PNPM_AUTH_COMMANDS_LOGIN_QR_CODE))]
    QrCode(#[error(source)] GenerateQrCodeError),

    #[display("The login request failed: {reason}")]
    #[diagnostic(code(ERR_PNPM_AUTH_COMMANDS_LOGIN_REQUEST_FAILED))]
    Request {
        #[error(not(source))]
        reason: String,
    },

    #[display("Failed to read the login prompt: {reason}")]
    #[diagnostic(code(ERR_PNPM_AUTH_COMMANDS_LOGIN_PROMPT_FAILED))]
    Prompt {
        #[error(not(source))]
        reason: String,
    },

    #[display("Failed to read auth.ini at {}: {error}", path.display())]
    #[diagnostic(code(ERR_PNPM_AUTH_COMMANDS_READ_AUTH_INI))]
    ReadAuthIni {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to write auth.ini at {}: {error}", path.display())]
    #[diagnostic(code(ERR_PNPM_AUTH_COMMANDS_WRITE_AUTH_INI))]
    WriteAuthIni {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}
