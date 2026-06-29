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
    assert!(message.contains("timed out"), "message should mention timing out, got {message:?}");
}
