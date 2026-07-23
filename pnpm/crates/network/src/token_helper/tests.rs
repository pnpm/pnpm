use super::{TokenHelperError, TokenHelperOutput, execute_token_helper};
use std::io;

fn ok_stdout(stdout: &str) -> io::Result<TokenHelperOutput> {
    Ok(TokenHelperOutput { success: true, stdout: stdout.to_owned(), stderr: String::new() })
}

#[test]
fn a_timed_out_runner_maps_to_the_timeout_error() {
    let command = vec!["helper".to_owned()];
    let error = execute_token_helper(&command, |_| Err(io::Error::from(io::ErrorKind::TimedOut)))
        .expect_err("a timed-out helper must fail");
    assert!(matches!(error, TokenHelperError::Timeout { .. }), "got {error:?}");
}

/// A helper that never returns is killed at the deadline instead of
/// hanging forever. Uses a real `sleep` (Unix) far longer than the
/// tiny test timeout, so the test only passes if the kill actually fires.
#[cfg(unix)]
#[test]
fn a_hung_command_is_killed_at_the_deadline() {
    use std::time::{Duration, Instant};

    let command = vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 10".to_owned()];
    let started = Instant::now();
    let result = super::run_token_helper_command_with_timeout(&command, Duration::from_millis(200));
    let elapsed = started.elapsed();

    assert_eq!(result.expect_err("must time out").kind(), io::ErrorKind::TimedOut);
    assert!(elapsed < Duration::from_secs(5), "returned only after {elapsed:?} — was it killed?");
}

#[test]
fn prepends_bearer_to_a_raw_token() {
    let command = vec!["helper".to_owned()];
    let header = execute_token_helper(&command, |_| ok_stdout("s3cr3t\n")).expect("token resolves");
    assert_eq!(header, "Bearer s3cr3t");
}

#[test]
fn keeps_a_token_that_already_carries_a_scheme() {
    let command = vec!["helper".to_owned()];
    let header = execute_token_helper(&command, |_| ok_stdout("Basic dXNlcjpwYXNz"))
        .expect("token resolves");
    assert_eq!(header, "Basic dXNlcjpwYXNz");
}

#[test]
fn passes_the_full_command_to_the_runner() {
    let command = vec!["helper".to_owned(), "--flag".to_owned(), "value".to_owned()];
    let header = execute_token_helper(&command, |received| {
        assert_eq!(received, ["helper", "--flag", "value"]);
        ok_stdout("tok")
    })
    .expect("token resolves");
    assert_eq!(header, "Bearer tok");
}

#[test]
fn a_non_zero_exit_is_an_error_status() {
    let command = vec!["helper".to_owned()];
    let error = execute_token_helper(&command, |_| {
        Ok(TokenHelperOutput { success: false, stdout: String::new(), stderr: "boom".to_owned() })
    })
    .expect_err("non-zero exit must fail");
    assert!(matches!(error, TokenHelperError::ErrorStatus { .. }), "got {error:?}");
}

#[test]
fn an_empty_token_is_an_error() {
    let command = vec!["helper".to_owned()];
    let error =
        execute_token_helper(&command, |_| ok_stdout("   \n")).expect_err("empty token must fail");
    assert!(matches!(error, TokenHelperError::EmptyToken { .. }), "got {error:?}");
}

#[test]
fn a_spawn_failure_is_surfaced() {
    let command = vec!["helper".to_owned()];
    let error = execute_token_helper(&command, |_| Err(io::Error::from(io::ErrorKind::NotFound)))
        .expect_err("spawn failure must surface");
    assert!(matches!(error, TokenHelperError::Spawn { .. }), "got {error:?}");
}
