use pacquet_diagnostics::miette::{self, Diagnostic};

/// Web-based authentication did not complete before the timeout.
///
/// Ports pnpm's `WebAuthTimeoutError`. The `code(...)` is part of the
/// public contract (<https://pnpm.io/errors>). `start_time` / `end_time`
/// are the Unix-epoch-millisecond [`Clock`](crate::Clock) readings that
/// bracketed the poll, and `timeout` is the configured budget in
/// milliseconds — the same three numbers pnpm's error carries.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
#[display("Web-based authentication timed out before it could be completed")]
#[diagnostic(
    code(ERR_PNPM_WEBAUTH_TIMEOUT),
    help(
        "Re-run this command and complete the authentication step in your browser before the time \
         limit is reached"
    )
)]
pub struct WebAuthTimeoutError {
    pub end_time: u64,
    pub start_time: u64,
    pub timeout: u64,
}

impl WebAuthTimeoutError {
    #[must_use]
    pub fn new(end_time: u64, start_time: u64, timeout: u64) -> Self {
        WebAuthTimeoutError { end_time, start_time, timeout }
    }
}

#[cfg(test)]
mod tests {
    use pacquet_diagnostics::miette::Diagnostic;
    use pretty_assertions::assert_eq;

    use super::WebAuthTimeoutError;

    #[test]
    fn stores_end_time_start_time_and_timeout() {
        let err = WebAuthTimeoutError::new(310_000, 10_000, 300_000);
        assert_eq!(err.end_time, 310_000);
        assert_eq!(err.start_time, 10_000);
        assert_eq!(err.timeout, 300_000);
    }

    #[test]
    fn has_webauth_timeout_code() {
        let err = WebAuthTimeoutError::new(0, 0, 0);
        assert_eq!(err.code().expect("a diagnostic code").to_string(), "ERR_PNPM_WEBAUTH_TIMEOUT");
    }

    #[test]
    fn includes_a_hint_about_re_running_the_command() {
        let err = WebAuthTimeoutError::new(0, 0, 0);
        let help = err.help().expect("a help hint").to_string();
        assert!(help.contains("Re-run"), "help should mention re-running, got {help:?}");
    }

    #[test]
    fn has_a_descriptive_message() {
        let err = WebAuthTimeoutError::new(0, 0, 0);
        let message = err.to_string();
        assert!(
            message.contains("timed out"),
            "message should mention timing out, got {message:?}"
        );
    }
}
