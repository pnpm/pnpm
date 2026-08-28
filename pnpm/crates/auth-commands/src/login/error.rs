use std::{io, path::PathBuf};

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_network_web_auth::{WebAuthTimeoutError, WithOtpError};
use pnpm_workspace_manifest_writer::EditManifestFieldError;

use super::classic_login::ClassicLoginOpError;
use crate::config_yaml::ParseConfigYamlError;

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
        "The registry returned an authentication URL containing control characters and was \
         rejected as a possible terminal-spoofing attempt"
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

    #[display("Failed to read the global config file at {}: {error}", path.display())]
    #[diagnostic(code(ERR_PNPM_AUTH_COMMANDS_READ_CONFIG_YAML))]
    ReadConfigYaml {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to write the global config file at {}: {error}", path.display())]
    #[diagnostic(code(ERR_PNPM_AUTH_COMMANDS_WRITE_CONFIG_YAML))]
    WriteConfigYaml {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Cannot record a login for this registry: {reason}")]
    #[diagnostic(code(ERR_PNPM_AUTH_COMMANDS_UNRECORDABLE_LOGIN))]
    UnrecordableLogin {
        #[error(not(source))]
        reason: String,
    },

    #[display("{_0}")]
    #[diagnostic(transparent)]
    ParseConfigYaml(ParseConfigYamlError),

    #[display("{_0}")]
    #[diagnostic(transparent)]
    EditConfigYaml(EditManifestFieldError),
}
