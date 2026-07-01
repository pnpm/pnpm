//! Port of [`otpEnv.ts`](https://github.com/pnpm/pnpm/blob/54c5c0e028/pnpm11/releasing/commands/src/publish/otpEnv.ts): let `PNPM_CONFIG_OTP` supply the one-time password
//! when none was passed explicitly.

use crate::capabilities::EnvVar;

const ENV_KEY: &str = "PNPM_CONFIG_OTP";

/// Resolve the effective OTP: an explicit `current_otp` always wins; otherwise
/// a non-empty `PNPM_CONFIG_OTP` is used. An empty string (in either place) is
/// treated as "not defined", matching the TS `Boolean(opts.otp) || !otp` test.
///
/// Ports TS `optionsWithOtpEnv`.
#[must_use]
pub fn resolve_otp_from_env<Sys: EnvVar>(current_otp: Option<String>) -> Option<String> {
    let current_otp = current_otp.filter(|otp| !otp.is_empty());
    if current_otp.is_some() {
        return current_otp;
    }
    match Sys::var(ENV_KEY) {
        Some(otp) if !otp.is_empty() => Some(otp),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
