//! Port of `otpEnv.ts`: let `PNPM_CONFIG_OTP` supply the one-time password
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
    if current_otp.as_deref().is_some_and(|otp| !otp.is_empty()) {
        return current_otp;
    }
    match Sys::var(ENV_KEY) {
        Some(otp) if !otp.is_empty() => Some(otp),
        _ => current_otp,
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_otp_from_env;
    use crate::capabilities::EnvVar;
    use pretty_assertions::assert_eq;

    struct NoEnv;
    impl EnvVar for NoEnv {
        fn var(_: &str) -> Option<String> {
            None
        }
    }

    struct EnvOtp;
    impl EnvVar for EnvOtp {
        fn var(name: &str) -> Option<String> {
            (name == "PNPM_CONFIG_OTP").then(|| "from-env".to_owned())
        }
    }

    struct EmptyEnvOtp;
    impl EnvVar for EmptyEnvOtp {
        fn var(_: &str) -> Option<String> {
            Some(String::new())
        }
    }

    #[test]
    fn explicit_otp_wins_over_env() {
        assert_eq!(
            resolve_otp_from_env::<EnvOtp>(Some("explicit".to_owned())),
            Some("explicit".to_owned())
        );
    }

    #[test]
    fn falls_back_to_env_when_unset() {
        assert_eq!(resolve_otp_from_env::<EnvOtp>(None), Some("from-env".to_owned()));
    }

    #[test]
    fn empty_explicit_otp_is_treated_as_unset() {
        assert_eq!(
            resolve_otp_from_env::<EnvOtp>(Some(String::new())),
            Some("from-env".to_owned())
        );
    }

    #[test]
    fn empty_env_otp_is_ignored() {
        assert_eq!(resolve_otp_from_env::<EmptyEnvOtp>(None), None);
    }

    #[test]
    fn no_env_keeps_current() {
        assert_eq!(resolve_otp_from_env::<NoEnv>(None), None);
    }
}
