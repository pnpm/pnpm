use std::{collections::HashMap, io, path::Path, sync::Mutex, time::Duration};

use pacquet_network::{RetryOpts, ThrottledClient};
use pacquet_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};

use super::{
    FsReadToString, FsWrite, LogoutError, LogoutOptions, RevokeOutcome, RevokeToken, logout,
};

fn no_retry() -> RetryOpts {
    RetryOpts { retries: 0, factor: 1, min_timeout: Duration::ZERO, max_timeout: Duration::ZERO }
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
