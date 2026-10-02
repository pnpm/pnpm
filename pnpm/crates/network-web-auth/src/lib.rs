//! Web-based authentication flow: QR-code display, token polling, and OTP
//! challenge handling.
//!
//! This is the Rust port of the TypeScript `@pnpm/network.web-auth`
//! package.

#[cfg(target_family = "wasm")]
extern crate pnpm_http as reqwest;

pub use capabilities::{
    Clock, EnterKeyListener, Host, OpenUrl, OpenUrlAndWait, PromptError, PromptOtp, Sleep,
    StdinIsTty, StdoutIsTty, WebAuthFetch, WebAuthFetchError,
};
pub use format_auth_url_message::{AuthUrlMessage, format_auth_url_message};
pub use generate_qr_code::{GenerateQrCodeError, generate_qr_code};
pub use poll_for_web_auth_token::{
    WebAuthFetchOptions, WebAuthFetchResponse, WebAuthRetryOptions, WebAuthTokenPollParams,
    body_may_carry_token, poll_for_web_auth_token,
};
pub use prompt_browser_open::prompt_browser_open;
pub use web_auth_timeout_error::WebAuthTimeoutError;
pub use with_otp_handling::{
    OtpChallenge, OtpError, OtpErrorBody, OtpNonInteractiveError, OtpSecondChallengeError,
    OtpSession, SyntheticOtpError, WithOtpError, otp_challenge_from_unauthorized_body,
    with_otp_handling,
};

mod capabilities;
mod format_auth_url_message;
mod generate_qr_code;
mod global_log;
mod poll_for_web_auth_token;
mod prompt_browser_open;
mod web_auth_timeout_error;
mod with_otp_handling;
