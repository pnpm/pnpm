//! Web-based authentication flow: QR-code display, token polling, and OTP
//! challenge handling.
//!
//! This is the Rust port of the TypeScript `@pnpm/network.web-auth`
//! package. It is shared infrastructure for the registry-auth commands:
//! `pnpm publish` drives its OTP challenges through this crate, and the
//! commands pacquet has not ported yet (`pnpm login` and friends) will
//! reuse the same flow when they land.
//!
//! # Dependency-injection seam
//!
//! The TypeScript package injects every side effect — the clock, the
//! sleep timer, `fetch`, the OTP prompt, the "press Enter" readline, the
//! browser opener — as a bag of closures on a `context` object. This crate
//! ports that seam to pacquet's convention: one `self`-less capability
//! trait per effect ([`Clock`], [`Sleep`], [`WebAuthFetch`], [`OpenUrl`],
//! [`EnterKeyListener`], [`PromptOtp`], and the [`StdinIsTty`] /
//! [`StdoutIsTty`] probes), composed as bounds on a single `Sys` type
//! parameter, with the real OS behind [`Host`] and `fn`-bound unit-struct
//! fakes in tests. User-facing messages flow through the `R: Reporter` seam
//! on pacquet's `pnpm:global` channel rather than a capability, matching
//! pnpm's `globalInfo` / `globalWarn`.

mod capabilities;
mod generate_qr_code;
mod global_log;
mod poll_for_web_auth_token;
mod prompt_browser_open;
mod web_auth_timeout_error;
mod with_otp_handling;

pub use capabilities::{
    Clock, EnterKeyListener, Host, OpenUrl, PromptError, PromptOtp, Sleep, StdinIsTty, StdoutIsTty,
    WebAuthFetch, WebAuthFetchError,
};
pub use generate_qr_code::{GenerateQrCodeError, generate_qr_code};
pub use poll_for_web_auth_token::{
    WebAuthFetchOptions, WebAuthFetchResponse, WebAuthRetryOptions, WebAuthTokenPollParams,
    poll_for_web_auth_token,
};
pub use prompt_browser_open::prompt_browser_open;
pub use web_auth_timeout_error::WebAuthTimeoutError;
pub use with_otp_handling::{
    OtpChallenge, OtpError, OtpErrorBody, OtpNonInteractiveError, OtpSecondChallengeError,
    SyntheticOtpError, WithOtpError, with_otp_handling,
};
