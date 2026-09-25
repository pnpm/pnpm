use super::{
    CaptureWriter, FsReadToString, FsWrite, Host, LogEvent, LogLevel, LogoutOptions, PnpmLog,
    Reporter, RetryOpts, RevokeOutcome, RevokeToken, ThrottledClient, auth_config, logout,
    no_retry, refused_local_addr, revoke_log_url, unused_client, warns,
};
use std::{io, path::Path, sync::Mutex, time::Duration};

#[test]
fn revoke_log_url_drops_the_token_segment() {
    assert_eq!(
        revoke_log_url("https://registry.npmjs.org/-/user/token/secret%2Ftoken"),
        "https://registry.npmjs.org/-/user/token",
    );
    // A URL with no `/` is returned unchanged rather than panicking.
    assert_eq!(revoke_log_url("token-only"), "token-only");
}

// The registry URL is attacker-influenced (a repo-controlled `.npmrc` or
// `--registry`): inline `user:pass@` credentials and terminal escape
// sequences must never reach stdout, warnings, or error messages.
#[tokio::test]
async fn not_logged_in_error_redacts_and_sanitizes_the_registry() {
    recording_reporter!(Rep, EVENTS);
    sys_fake!(
        Sys,
        writes = WRITES,
        revokes = REVOKES,
        read = { unreachable!() },
        revoke = unreachable!(),
    );
    let auth = auth_config(&[]);
    let err = logout::<Sys, Rep>(
        &unused_client(),
        LogoutOptions {
            registry: Some("https://user:s3cret@npm.example.com/\u{7}"),
            auth_config: &auth,
            config_dir: Path::new("/mock/config"),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .unwrap_err();

    let message = err.to_string();
    assert!(!message.contains("s3cret"), "credentials must be redacted: {message:?}");
    assert!(!message.contains('\u{7}'), "control characters must be stripped: {message:?}");
    assert!(message.contains("npm.example.com"), "host should remain: {message:?}");
}

#[tokio::test]
async fn success_message_and_warning_redact_the_registry() {
    recording_reporter!(Rep, EVENTS);
    sys_fake!(
        Sys,
        writes = WRITES,
        revokes = REVOKES,
        read = { Ok(String::new()) },
        revoke = RevokeOutcome::Revoked,
    );
    // `nerf_dart` drops the userinfo and query, so the token key is the same as
    // for a credential-free registry. The escape sequence sits in the query so
    // it survives credential redaction and must be removed by sanitization.
    let auth = auth_config(&[("//npm.example.com/:_authToken", "tok")]);
    let result = logout::<Sys, Rep>(
        &unused_client(),
        LogoutOptions {
            registry: Some("https://user:s3cret@npm.example.com/?e=\u{1b}[31m"),
            auth_config: &auth,
            config_dir: Path::new("/config"),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .unwrap();

    for output in [&result, &warns(&EVENTS).remove(0)] {
        assert!(output.contains("https://npm.example.com/"), "host should remain: {output:?}");
        assert!(!output.contains("s3cret"), "credentials must be redacted: {output:?}");
        assert!(!output.contains('\u{1b}'), "control characters must be stripped: {output:?}");
    }
}

// Regression for the token leaking into retry logs: the revoke URL carries
// the token in its path, and `send_with_retry` logs the URL it routes on
// plus the `reqwest` error (which echoes the request URL). A retryable
// failure must not write the token to the logs.
#[tokio::test]
async fn retry_logs_do_not_leak_the_token() {
    const TOKEN: &str = "SUPERSECRETTOKEN";
    // The connect fails at once, so the single retry fires a warn log.
    let revoke_url = format!("http://{}/-/user/token/{TOKEN}", refused_local_addr());
    let retry = RetryOpts {
        retries: 1,
        factor: 1,
        min_timeout: Duration::ZERO,
        max_timeout: Duration::ZERO,
    };

    let buffer = std::sync::Arc::new(Mutex::new(Vec::<u8>::new()));
    let subscriber = tracing_subscriber::fmt()
        .with_writer(CaptureWriter(std::sync::Arc::clone(&buffer)))
        .with_max_level(tracing::Level::WARN)
        .finish();
    let outcome = {
        let _guard = tracing::subscriber::set_default(subscriber);
        Host::revoke(&ThrottledClient::new_for_installs(), &revoke_url, TOKEN, retry).await
    };

    assert_eq!(outcome, RevokeOutcome::Unreachable);
    let logs = String::from_utf8(buffer.lock().unwrap().clone()).expect("logs are UTF-8");
    assert!(logs.contains("retrying"), "a retry warn should have been logged: {logs:?}");
    assert!(!logs.contains(TOKEN), "the token must not appear in retry logs: {logs:?}");
}
