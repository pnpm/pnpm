use std::{collections::HashMap, io, path::Path, sync::Mutex, time::Duration};

use pacquet_network::{RetryOpts, ThrottledClient, nerf_dart};
use pacquet_reporter::{LogEvent, LogLevel, PnpmLog, Reporter, SilentReporter};
use tempfile::TempDir;

use super::{
    FsReadToString, FsWrite, Host, LogoutError, LogoutOptions, RevokeOutcome, RevokeToken, logout,
    revoke_log_url,
};

fn no_retry() -> RetryOpts {
    RetryOpts { retries: 0, factor: 1, min_timeout: Duration::ZERO, max_timeout: Duration::ZERO }
}

/// A `127.0.0.1:<port>` address guaranteed to refuse connections: bind an
/// ephemeral port, then drop the listener so the OS frees it. Deterministic
/// across environments, unlike assuming a fixed low port is closed.
fn refused_local_addr() -> String {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral port");
    let addr = listener.local_addr().expect("read local addr");
    drop(listener);
    addr.to_string()
}

/// A throwaway HTTP client. Every test fakes [`RevokeToken`], so the
/// client is never used to send — it only satisfies [`logout`]'s
/// signature.
fn unused_client() -> ThrottledClient {
    ThrottledClient::new_for_installs()
}

fn auth_config(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(key, value)| ((*key).to_string(), (*value).to_string())).collect()
}

/// Declare a per-test [`Reporter`] fake recording every `pnpm` log line
/// into a function-scoped `static`, so parallel tests never share a
/// buffer.
macro_rules! recording_reporter {
    ($reporter:ident, $buffer:ident) => {
        static $buffer: Mutex<Vec<(LogLevel, String)>> = Mutex::new(Vec::new());
        $buffer.lock().unwrap().clear();
        struct $reporter;
        impl Reporter for $reporter {
            fn emit(event: &LogEvent) {
                if let LogEvent::Pnpm(PnpmLog { level, message, .. }) = event {
                    $buffer.lock().unwrap().push((*level, message.clone()));
                }
            }
        }
    };
}

/// Declare a per-test `Sys` fake: `read` is the `auth.ini` read result,
/// `revoke` is the canned revocation outcome. Writes and revoke calls
/// are captured into the named function-scoped `static`s.
macro_rules! sys_fake {
    ($sys:ident, writes = $writes:ident, revokes = $revokes:ident, read = $read:block, revoke = $revoke:expr $(,)?) => {
        static $writes: Mutex<Vec<(std::path::PathBuf, String)>> = Mutex::new(Vec::new());
        static $revokes: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
        $writes.lock().unwrap().clear();
        $revokes.lock().unwrap().clear();
        struct $sys;
        impl FsReadToString for $sys {
            fn read_to_string(_path: &Path) -> io::Result<String> $read
        }
        impl FsWrite for $sys {
            fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
                let text = String::from_utf8(bytes.to_vec()).expect("written auth.ini is UTF-8");
                $writes.lock().unwrap().push((path.to_path_buf(), text));
                Ok(())
            }
        }
        impl RevokeToken for $sys {
            async fn revoke(
                _http_client: &ThrottledClient,
                revoke_url: &str,
                token: &str,
                _retry: RetryOpts,
            ) -> RevokeOutcome {
                $revokes.lock().unwrap().push((revoke_url.to_string(), token.to_string()));
                $revoke
            }
        }
    };
}

fn infos(buffer: &Mutex<Vec<(LogLevel, String)>>) -> Vec<String> {
    buffer
        .lock()
        .unwrap()
        .iter()
        .filter(|(level, _)| *level == LogLevel::Info)
        .map(|(_, message)| message.clone())
        .collect()
}

fn warns(buffer: &Mutex<Vec<(LogLevel, String)>>) -> Vec<String> {
    buffer
        .lock()
        .unwrap()
        .iter()
        .filter(|(level, _)| *level == LogLevel::Warn)
        .map(|(_, message)| message.clone())
        .collect()
}

#[tokio::test]
async fn throws_when_not_logged_in() {
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
            registry: None,
            auth_config: &auth,
            config_dir: Path::new("/mock/config"),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, LogoutError::NotLoggedIn { .. }));
    assert_eq!(err.to_string(), "Not logged in to https://registry.npmjs.org/, so can't log out");
    assert_eq!(
        miette::Diagnostic::code(&err).map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_NOT_LOGGED_IN"),
    );
}

#[tokio::test]
async fn throws_when_not_logged_in_to_a_custom_registry() {
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
            registry: Some("https://npm.example.com/"),
            auth_config: &auth,
            config_dir: Path::new("/mock/config"),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err.to_string(), "Not logged in to https://npm.example.com/, so can't log out");
}

#[tokio::test]
async fn revokes_token_on_registry_and_removes_from_auth_ini() {
    recording_reporter!(Rep, EVENTS);
    sys_fake!(
        Sys,
        writes = WRITES,
        revokes = REVOKES,
        read = {
            Ok("//registry.npmjs.org/:_authToken=my-token-123\nother-setting=value\n".to_string())
        },
        revoke = RevokeOutcome::Revoked,
    );
    let auth = auth_config(&[("//registry.npmjs.org/:_authToken", "my-token-123")]);
    let result = logout::<Sys, Rep>(
        &unused_client(),
        LogoutOptions {
            registry: None,
            auth_config: &auth,
            config_dir: Path::new("/custom/config"),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .unwrap();

    assert_eq!(result, "Logged out of https://registry.npmjs.org/");
    let revokes = REVOKES.lock().unwrap();
    assert_eq!(
        revokes.as_slice(),
        [(
            "https://registry.npmjs.org/-/user/token/my-token-123".to_string(),
            "my-token-123".to_string(),
        )],
    );
    let writes = WRITES.lock().unwrap();
    let (path, text) = writes.first().expect("auth.ini was written");
    assert_eq!(path, Path::new("/custom/config/auth.ini"));
    assert_eq!(text, "other-setting=value\n");
}

#[tokio::test]
async fn logs_out_from_a_custom_registry() {
    recording_reporter!(Rep, EVENTS);
    sys_fake!(
        Sys,
        writes = WRITES,
        revokes = REVOKES,
        read = { Ok("//npm.example.com/:_authToken=custom-token\n".to_string()) },
        revoke = RevokeOutcome::Revoked,
    );
    let auth = auth_config(&[("//npm.example.com/:_authToken", "custom-token")]);
    let result = logout::<Sys, Rep>(
        &unused_client(),
        LogoutOptions {
            registry: Some("https://npm.example.com/"),
            auth_config: &auth,
            config_dir: Path::new("/config"),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .unwrap();

    assert_eq!(result, "Logged out of https://npm.example.com/");
    let revokes = REVOKES.lock().unwrap();
    assert_eq!(revokes[0].0, "https://npm.example.com/-/user/token/custom-token");
    let writes = WRITES.lock().unwrap();
    assert_eq!(writes[0].1, "");
}

#[tokio::test]
async fn removes_token_locally_when_registry_returns_non_ok() {
    recording_reporter!(Rep, EVENTS);
    sys_fake!(
        Sys,
        writes = WRITES,
        revokes = REVOKES,
        read = { Ok("//registry.npmjs.org/:_authToken=old-token\n".to_string()) },
        revoke = RevokeOutcome::Rejected { status: 404 },
    );
    let auth = auth_config(&[("//registry.npmjs.org/:_authToken", "old-token")]);
    let result = logout::<Sys, Rep>(
        &unused_client(),
        LogoutOptions {
            registry: None,
            auth_config: &auth,
            config_dir: Path::new("/config"),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .unwrap();

    assert_eq!(result, "Logged out of https://registry.npmjs.org/");
    assert_eq!(WRITES.lock().unwrap()[0].1, "");
    assert_eq!(infos(&EVENTS), ["Registry returned HTTP 404 when revoking token"]);
}

#[tokio::test]
async fn removes_token_locally_when_fetch_errors() {
    recording_reporter!(Rep, EVENTS);
    sys_fake!(
        Sys,
        writes = WRITES,
        revokes = REVOKES,
        read = { Ok("//registry.npmjs.org/:_authToken=net-err-token\n".to_string()) },
        revoke = RevokeOutcome::Unreachable,
    );
    let auth = auth_config(&[("//registry.npmjs.org/:_authToken", "net-err-token")]);
    let result = logout::<Sys, Rep>(
        &unused_client(),
        LogoutOptions {
            registry: None,
            auth_config: &auth,
            config_dir: Path::new("/config"),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .unwrap();

    assert_eq!(result, "Logged out of https://registry.npmjs.org/");
    assert_eq!(WRITES.lock().unwrap()[0].1, "");
    assert_eq!(infos(&EVENTS), ["Could not reach the registry to revoke the token"]);
}

#[tokio::test]
async fn warns_when_token_is_not_in_auth_ini() {
    recording_reporter!(Rep, EVENTS);
    sys_fake!(
        Sys,
        writes = WRITES,
        revokes = REVOKES,
        read = { Ok(String::new()) },
        revoke = RevokeOutcome::Revoked,
    );
    let auth = auth_config(&[("//registry.npmjs.org/:_authToken", "npmrc-only-token")]);
    let result = logout::<Sys, Rep>(
        &unused_client(),
        LogoutOptions {
            registry: None,
            auth_config: &auth,
            config_dir: Path::new("/config"),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .unwrap();

    assert_eq!(result, "Logged out of https://registry.npmjs.org/");
    assert!(WRITES.lock().unwrap().is_empty(), "auth.ini must not be written");
    let warnings = warns(&EVENTS);
    let warning = warnings.first().expect("a warning was emitted");
    let expected_path = Path::new("/config").join("auth.ini");
    assert!(warning.contains(&format!("was not found in {}", expected_path.display())));
    assert!(warning.contains("The token was revoked on the registry but must be removed manually"));
}

#[tokio::test]
async fn throws_when_registry_call_fails_and_token_not_in_auth_ini() {
    recording_reporter!(Rep, EVENTS);
    sys_fake!(
        Sys,
        writes = WRITES,
        revokes = REVOKES,
        read = { Ok(String::new()) },
        revoke = RevokeOutcome::Rejected { status: 401 },
    );
    let auth = auth_config(&[("//registry.npmjs.org/:_authToken", "orphan-token")]);
    let err = logout::<Sys, Rep>(
        &unused_client(),
        LogoutOptions {
            registry: None,
            auth_config: &auth,
            config_dir: Path::new("/config"),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .unwrap_err();

    assert!(matches!(err, LogoutError::LogoutFailed { .. }));
    assert_eq!(
        miette::Diagnostic::code(&err).map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_LOGOUT_FAILED"),
    );
    let message = err.to_string();
    assert!(message.contains("Failed to log out of https://registry.npmjs.org/"));
    assert!(message.contains("may still need to be revoked on the registry"));
    assert_eq!(infos(&EVENTS), ["Registry returned HTTP 401 when revoking token"]);
}

#[tokio::test]
async fn warns_when_auth_ini_does_not_exist() {
    recording_reporter!(Rep, EVENTS);
    sys_fake!(
        Sys,
        writes = WRITES,
        revokes = REVOKES,
        read = { Err(io::Error::new(io::ErrorKind::NotFound, "ENOENT")) },
        revoke = RevokeOutcome::Revoked,
    );
    let auth = auth_config(&[("//registry.npmjs.org/:_authToken", "token-in-npmrc")]);
    let result = logout::<Sys, Rep>(
        &unused_client(),
        LogoutOptions {
            registry: None,
            auth_config: &auth,
            config_dir: Path::new("/nonexistent/config"),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .unwrap();

    assert_eq!(result, "Logged out of https://registry.npmjs.org/");
    let warnings = warns(&EVENTS);
    let expected_path = Path::new("/nonexistent/config").join("auth.ini");
    assert!(warnings[0].contains(&format!("was not found in {}", expected_path.display())));
}

#[tokio::test]
async fn propagates_non_not_found_read_errors() {
    recording_reporter!(Rep, EVENTS);
    sys_fake!(
        Sys,
        writes = WRITES,
        revokes = REVOKES,
        read = { Err(io::Error::new(io::ErrorKind::PermissionDenied, "EACCES")) },
        revoke = RevokeOutcome::Revoked,
    );
    let auth = auth_config(&[("//registry.npmjs.org/:_authToken", "some-token")]);
    let err = logout::<Sys, Rep>(
        &unused_client(),
        LogoutOptions {
            registry: None,
            auth_config: &auth,
            config_dir: Path::new("/broken/config"),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .unwrap_err();

    let LogoutError::ReadAuthIni { error, .. } = &err else {
        panic!("expected ReadAuthIni, got {err:?}");
    };
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}

#[tokio::test]
async fn url_encodes_the_token_when_revoking() {
    recording_reporter!(Rep, EVENTS);
    sys_fake!(
        Sys,
        writes = WRITES,
        revokes = REVOKES,
        read = { Ok(String::new()) },
        revoke = RevokeOutcome::Revoked,
    );
    let auth = auth_config(&[("//registry.npmjs.org/:_authToken", "token/with+special=chars")]);
    logout::<Sys, Rep>(
        &unused_client(),
        LogoutOptions {
            registry: None,
            auth_config: &auth,
            config_dir: Path::new("/config"),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .unwrap();

    assert_eq!(
        REVOKES.lock().unwrap()[0].0,
        "https://registry.npmjs.org/-/user/token/token%2Fwith%2Bspecial%3Dchars",
    );
}

#[tokio::test]
async fn normalizes_the_registry_url() {
    recording_reporter!(Rep, EVENTS);
    sys_fake!(
        Sys,
        writes = WRITES,
        revokes = REVOKES,
        read = { Ok("//example.org/:_authToken=tok\n".to_string()) },
        revoke = RevokeOutcome::Revoked,
    );
    let auth = auth_config(&[("//example.org/:_authToken", "tok")]);
    let result = logout::<Sys, Rep>(
        &unused_client(),
        LogoutOptions {
            registry: Some("https://example.org"),
            auth_config: &auth,
            config_dir: Path::new("/config"),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .unwrap();

    assert_eq!(result, "Logged out of https://example.org/");
    assert_eq!(WRITES.lock().unwrap()[0].1, "");
}

#[tokio::test]
async fn handles_registry_with_a_path() {
    recording_reporter!(Rep, EVENTS);
    sys_fake!(
        Sys,
        writes = WRITES,
        revokes = REVOKES,
        read = { Ok("//example.com/npm/:_authToken=path-token\n".to_string()) },
        revoke = RevokeOutcome::Revoked,
    );
    let auth = auth_config(&[("//example.com/npm/:_authToken", "path-token")]);
    let result = logout::<Sys, Rep>(
        &unused_client(),
        LogoutOptions {
            registry: Some("https://example.com/npm/"),
            auth_config: &auth,
            config_dir: Path::new("/config"),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .unwrap();

    assert_eq!(result, "Logged out of https://example.com/npm/");
    assert_eq!(REVOKES.lock().unwrap()[0].0, "https://example.com/npm/-/user/token/path-token");
    assert_eq!(WRITES.lock().unwrap()[0].1, "");
}

#[tokio::test]
async fn handles_registry_under_a_subpath_without_a_trailing_slash() {
    recording_reporter!(Rep, EVENTS);
    sys_fake!(
        Sys,
        writes = WRITES,
        revokes = REVOKES,
        read = { Ok("//example.com/npm/registry/:_authToken=subpath-token\n".to_string()) },
        revoke = RevokeOutcome::Revoked,
    );
    let auth = auth_config(&[("//example.com/npm/registry/:_authToken", "subpath-token")]);
    let result = logout::<Sys, Rep>(
        &unused_client(),
        LogoutOptions {
            registry: Some("https://example.com/npm/registry"),
            auth_config: &auth,
            config_dir: Path::new("/config"),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .unwrap();

    assert_eq!(result, "Logged out of https://example.com/npm/registry/");
    assert_eq!(
        REVOKES.lock().unwrap()[0].0,
        "https://example.com/npm/registry/-/user/token/subpath-token"
    );
    assert_eq!(WRITES.lock().unwrap()[0].1, "");
}

#[tokio::test]
async fn propagates_auth_ini_write_errors() {
    recording_reporter!(Rep, EVENTS);
    // `sys_fake!`'s write always succeeds, so this branch needs a
    // hand-written fake whose `FsWrite::write` returns an error.
    struct Sys;
    impl FsReadToString for Sys {
        fn read_to_string(_path: &Path) -> io::Result<String> {
            Ok("//registry.npmjs.org/:_authToken=tok\n".to_string())
        }
    }
    impl FsWrite for Sys {
        fn write(_path: &Path, _bytes: &[u8]) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "EACCES"))
        }
    }
    impl RevokeToken for Sys {
        async fn revoke(
            _http_client: &ThrottledClient,
            _revoke_url: &str,
            _token: &str,
            _retry: RetryOpts,
        ) -> RevokeOutcome {
            RevokeOutcome::Revoked
        }
    }
    let auth = auth_config(&[("//registry.npmjs.org/:_authToken", "tok")]);
    let err = logout::<Sys, Rep>(
        &unused_client(),
        LogoutOptions {
            registry: None,
            auth_config: &auth,
            config_dir: Path::new("/config"),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .unwrap_err();

    let LogoutError::WriteAuthIni { path, error } = &err else {
        panic!("expected WriteAuthIni, got {err:?}");
    };
    assert_eq!(path, &Path::new("/config").join("auth.ini"));
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}

// The tests below drive the production `Host` provider end-to-end —
// real filesystem reads/writes of `auth.ini` and a real `DELETE` over
// the HTTP stack — against a `tempfile::TempDir` and a `mockito` server.
// They cover the side-effecting code the `Sys`-fake tests above
// deliberately bypass.

#[tokio::test]
async fn host_revokes_and_removes_token() {
    const TOKEN: &str = "secret-token";
    let mut server = mockito::Server::new_async().await;
    let mock =
        server.mock("DELETE", "/-/user/token/secret-token").with_status(200).create_async().await;
    let registry = server.url();
    let token_key = format!("{}:_authToken", nerf_dart(&format!("{registry}/")));

    let config_dir = TempDir::new().expect("create temp config dir");
    std::fs::write(
        config_dir.path().join("auth.ini"),
        format!("{token_key}={TOKEN}\nother=keep\n"),
    )
    .expect("seed auth.ini");
    let auth_config = auth_config(&[(&token_key, TOKEN)]);

    let result = logout::<Host, SilentReporter>(
        &ThrottledClient::new_for_installs(),
        LogoutOptions {
            registry: Some(&registry),
            auth_config: &auth_config,
            config_dir: config_dir.path(),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .expect("logout succeeds");

    mock.assert_async().await;
    assert_eq!(result, format!("Logged out of {registry}/"));
    let remaining =
        std::fs::read_to_string(config_dir.path().join("auth.ini")).expect("read auth.ini");
    assert!(!remaining.contains(TOKEN), "token should be gone: {remaining:?}");
    assert!(remaining.contains("other=keep"), "other settings kept: {remaining:?}");
}

#[tokio::test]
async fn host_removes_token_locally_when_registry_rejects() {
    const TOKEN: &str = "old-token";
    let mut server = mockito::Server::new_async().await;
    let mock =
        server.mock("DELETE", "/-/user/token/old-token").with_status(404).create_async().await;
    let registry = server.url();
    let token_key = format!("{}:_authToken", nerf_dart(&format!("{registry}/")));

    let config_dir = TempDir::new().expect("create temp config dir");
    std::fs::write(config_dir.path().join("auth.ini"), format!("{token_key}={TOKEN}\n"))
        .expect("seed auth.ini");
    let auth_config = auth_config(&[(&token_key, TOKEN)]);

    let result = logout::<Host, SilentReporter>(
        &ThrottledClient::new_for_installs(),
        LogoutOptions {
            registry: Some(&registry),
            auth_config: &auth_config,
            config_dir: config_dir.path(),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .expect("logout still removes the local token");

    mock.assert_async().await;
    assert_eq!(result, format!("Logged out of {registry}/"));
    let remaining =
        std::fs::read_to_string(config_dir.path().join("auth.ini")).expect("read auth.ini");
    assert!(!remaining.contains(TOKEN), "token should be gone: {remaining:?}");
}

#[tokio::test]
async fn host_removes_token_locally_when_registry_unreachable() {
    const TOKEN: &str = "net-err-token";
    let registry = format!("http://{}", refused_local_addr());
    let registry = registry.as_str();
    let token_key = format!("{}:_authToken", nerf_dart(&format!("{registry}/")));

    let config_dir = TempDir::new().expect("create temp config dir");
    std::fs::write(config_dir.path().join("auth.ini"), format!("{token_key}={TOKEN}\n"))
        .expect("seed auth.ini");
    let auth_config = auth_config(&[(&token_key, TOKEN)]);

    let result = logout::<Host, SilentReporter>(
        &ThrottledClient::new_for_installs(),
        LogoutOptions {
            registry: Some(registry),
            auth_config: &auth_config,
            config_dir: config_dir.path(),
            retry: no_retry(),
            prefix: "/mock",
        },
    )
    .await
    .expect("logout still removes the local token");

    assert_eq!(result, format!("Logged out of {registry}/"));
    let remaining =
        std::fs::read_to_string(config_dir.path().join("auth.ini")).expect("read auth.ini");
    assert!(!remaining.contains(TOKEN), "token should be gone: {remaining:?}");
}

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
    // A closed local port refuses the connection at once, so the single retry
    // fires a warn log.
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

#[derive(Clone)]
struct CaptureWriter(std::sync::Arc<Mutex<Vec<u8>>>);

impl io::Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CaptureWriter {
    type Writer = CaptureWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}
