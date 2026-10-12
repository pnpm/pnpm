use derive_more::{Display, Error};
use miette::Diagnostic;

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ProfileError {
    #[display("You must be logged in to view or change your profile")]
    #[diagnostic(code(ERR_PNPM_PROFILE_UNAUTHORIZED))]
    Unauthorized,

    #[display("This operation requires a one-time password. Pass it with --otp <code>.")]
    #[diagnostic(code(ERR_PNPM_PROFILE_OTP_REQUIRED))]
    OtpRequired,

    #[display(
        "Subcommand is required (get, set, enable-2fa, disable-2fa). Use `pnpm profile get` to view your profile."
    )]
    #[diagnostic(code(ERR_PNPM_PROFILE_SUBCOMMAND_REQUIRED))]
    SubcommandRequired,

    #[display("Unknown profile command: {command}")]
    #[diagnostic(code(ERR_PNPM_PROFILE_UNKNOWN_COMMAND))]
    UnknownCommand {
        #[error(not(source))]
        command: String,
    },

    #[display(
        r#""{property}" is not a property we can set. Valid properties are: email, password, fullname, homepage, freenode, twitter, github"#
    )]
    #[diagnostic(code(ERR_PNPM_PROFILE_INVALID_PROPERTY))]
    InvalidProperty {
        #[error(not(source))]
        property: String,
    },

    #[display("pnpm profile set <prop> <value>")]
    #[diagnostic(code(ERR_PNPM_PROFILE_ARGS_REQUIRED))]
    SetArgsRequired,

    #[display(
        "pnpm profile set password\nDo not include your current or new passwords on the command line."
    )]
    #[diagnostic(code(ERR_PNPM_PROFILE_PASSWORD_ON_COMMAND_LINE))]
    PasswordOnCommandLine,

    #[display("pnpm profile enable-2fa [auth-and-writes|auth-only]")]
    #[diagnostic(code(ERR_PNPM_PROFILE_ARGS_REQUIRED))]
    EnableTfaArgs,

    #[display(
        r#"Invalid two-factor authentication mode "{mode}".
Valid modes are:
  auth-only - Require two-factor authentication only when logging in
  auth-and-writes - Require two-factor authentication when logging in AND when publishing"#
    )]
    #[diagnostic(code(ERR_PNPM_PROFILE_INVALID_2FA_MODE))]
    Invalid2faMode {
        #[error(not(source))]
        mode: String,
    },

    #[display(
        "Enabling two-factor authentication is an interactive operation and {format} output mode is not available"
    )]
    #[diagnostic(code(ERR_PNPM_PROFILE_2FA_INTERACTIVE))]
    TfaInteractive {
        #[error(not(source))]
        format: &'static str,
    },

    #[display(
        "Unknown error enabling two-factor authentication. Expected otpauth URL, got: {response}"
    )]
    #[diagnostic(code(ERR_PNPM_PROFILE_2FA_UNEXPECTED_RESPONSE))]
    TfaUnexpectedResponse {
        #[error(not(source))]
        response: String,
    },

    #[display("A one-time password from your authenticator is required to finish enabling 2FA")]
    #[diagnostic(code(ERR_PNPM_PROFILE_2FA_OTP_MISSING))]
    TfaOtpMissing,

    #[display("The registry's profile response exceeds {limit} bytes")]
    #[diagnostic(code(ERR_PNPM_PROFILE_RESPONSE_TOO_LARGE))]
    ResponseTooLarge { limit: usize },

    #[display("You do not have permission to view or modify this profile. {body}")]
    #[diagnostic(code(ERR_PNPM_FORBIDDEN))]
    Forbidden {
        #[error(not(source))]
        body: String,
    },

    #[display("Failed to {operation}: {status} {status_text}. {body}")]
    #[diagnostic(code(ERR_PNPM_REGISTRY_ERROR))]
    RegistryFailed {
        #[error(not(source))]
        operation: &'static str,
        status: u16,
        #[error(not(source))]
        status_text: String,
        #[error(not(source))]
        body: String,
    },
}
