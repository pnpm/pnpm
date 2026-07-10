//! `login` tests for the web-login path: the registry `POST` returns 200 (or a
//! web-login error), so login never falls back to the classic `PUT` flow.

use std::{
    cell::RefCell,
    io,
    path::{Path, PathBuf},
    sync::Mutex,
};

use pacquet_network::nerf_dart;
use pacquet_network_web_auth_testing::{SleepBehavior, ok_202, ok_token, web_auth_fake};
use pretty_assertions::assert_eq;

use super::{
    LoginError, login,
    test_support::{PromptScript, ReadScript, client, login_fake, opts, written_settings},
};

#[tokio::test]
async fn should_use_web_login_when_registry_supports_it() {
    web_auth_fake!();
    login_fake!(FakeHost, login_writes);
    reset();
    reset_login();
    set_fetch(Box::new(|| Ok(ok_token("web-auth-token-123"))));

    let mut server = mockito::Server::new_async().await;
    let login_mock = server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(r#"{"loginUrl":"https://example.com/auth/login","doneUrl":"https://example.com/auth/done"}"#)
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/custom/config");

    let result = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .expect("web login succeeds");

    login_mock.assert_async().await;
    assert_eq!(result, format!("Logged in on {registry}/"));

    let writes = login_writes();
    let (path, _) = writes.first().expect("auth.ini was written");
    assert_eq!(path, &config_dir.join("auth.ini"));
    let token_key = format!("{}:_authToken", nerf_dart(&format!("{registry}/")));
    assert_eq!(written_settings(&writes).get(&token_key), Some("web-auth-token-123"));

    let messages = infos();
    assert_eq!(messages.len(), 2, "expected the auth-URL and Press-ENTER lines: {messages:?}");
    assert!(messages[0].contains("https://example.com/auth/login"), "got {messages:?}");
    assert_eq!(messages[1], "Press ENTER to open the URL in your browser.");
}

#[tokio::test]
async fn should_persist_a_scoped_auth_token_and_scope_registry_mapping() {
    web_auth_fake!();
    login_fake!(FakeHost, login_writes);
    reset();
    reset_login();
    set_fetch(Box::new(|| Ok(ok_token("scoped-token"))));

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(r#"{"loginUrl":"https://my-org.example/auth/login","doneUrl":"https://my-org.example/auth/done"}"#)
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/mock/config");

    let mut options = opts(&registry, config_dir);
    options.scope = Some("my-org");
    let result =
        login::<FakeHost, RecordingReporter>(&client(), options).await.expect("scoped login");

    assert_eq!(result, format!("Logged in on {registry}/"));
    let writes = login_writes();
    let settings = written_settings(&writes);
    let config_key = nerf_dart(&format!("{registry}/"));
    assert_eq!(settings.get(&format!("{config_key}:@my-org:_authToken")), Some("scoped-token"));
    assert_eq!(settings.get("@my-org:registry"), Some(format!("{registry}/").as_str()));
    assert_eq!(settings.get(&format!("{config_key}:_authToken")), None);
}

#[tokio::test]
async fn should_persist_scoped_auth_tokens_under_path_registries() {
    web_auth_fake!();
    login_fake!(FakeHost, login_writes);
    reset();
    reset_login();
    set_fetch(Box::new(|| Ok(ok_token("path-scoped-token"))));

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/npm/-/v1/login")
        .with_status(200)
        .with_body(r#"{"loginUrl":"https://example.com/auth/login","doneUrl":"https://example.com/auth/done"}"#)
        .create_async()
        .await;
    let registry = format!("{}/npm/", server.url());
    let config_dir = Path::new("/mock/config");

    let mut options = opts(&registry, config_dir);
    options.scope = Some("@team");
    let result =
        login::<FakeHost, RecordingReporter>(&client(), options).await.expect("path-scoped login");

    assert_eq!(result, format!("Logged in on {registry}"));
    let writes = login_writes();
    let settings = written_settings(&writes);
    let config_key = nerf_dart(&registry);
    assert_eq!(settings.get(&format!("{config_key}:@team:_authToken")), Some("path-scoped-token"));
    assert_eq!(settings.get("@team:registry"), Some(registry.as_str()));
    assert_eq!(settings.get(&format!("{config_key}:_authToken")), None);
}

#[tokio::test]
async fn should_accept_scope_with_a_leading_at_and_not_double_prefix() {
    web_auth_fake!();
    login_fake!(FakeHost, login_writes);
    reset();
    reset_login();
    set_fetch(Box::new(|| Ok(ok_token("tok"))));

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(r#"{"loginUrl":"https://my-org.example/auth/login","doneUrl":"https://my-org.example/auth/done"}"#)
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/mock/config");

    let mut options = opts(&registry, config_dir);
    options.scope = Some("@my-org");
    login::<FakeHost, RecordingReporter>(&client(), options).await.expect("scoped login");

    let writes = login_writes();
    let settings = written_settings(&writes);
    let config_key = nerf_dart(&format!("{registry}/"));
    assert_eq!(settings.get(&format!("{config_key}:@my-org:_authToken")), Some("tok"));
    assert_eq!(settings.get("@my-org:registry"), Some(format!("{registry}/").as_str()));
    assert_eq!(settings.get("@@my-org:registry"), None);
}

#[tokio::test]
async fn should_not_write_a_scope_mapping_when_scope_is_omitted() {
    web_auth_fake!();
    login_fake!(FakeHost, login_writes);
    reset();
    reset_login();
    set_fetch(Box::new(|| Ok(ok_token("tok"))));

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(r#"{"loginUrl":"https://example.com/auth/login","doneUrl":"https://example.com/auth/done"}"#)
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/mock/config");

    login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .expect("login");

    let writes = login_writes();
    let (_, text) = writes.first().expect("auth.ini was written");
    for line in text.lines() {
        assert!(!line.starts_with('@'), "no scope key expected, got line {line:?}");
    }
}

#[tokio::test]
async fn should_throw_when_web_login_returns_invalid_response() {
    web_auth_fake!();
    login_fake!(FakeHost);
    reset();
    reset_login();

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(r#"{"loginUrl":"https://example.org/auth"}"#)
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/mock/config");

    let err = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .unwrap_err();

    assert!(matches!(err, LoginError::InvalidResponse), "got {err:?}");
    assert_eq!(
        miette::Diagnostic::code(&err).map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_LOGIN_INVALID_RESPONSE"),
    );
    assert_eq!(err.to_string(), "The registry returned an invalid response for web-based login");
}

#[tokio::test]
async fn should_succeed_when_config_file_does_not_exist() {
    web_auth_fake!();
    login_fake!(FakeHost, set_ini_read, login_writes);
    reset();
    reset_login();
    set_fetch(Box::new(|| Ok(ok_token("new-token"))));
    set_ini_read(Box::new(|_| Err(io::Error::new(io::ErrorKind::NotFound, "ENOENT"))));

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(r#"{"loginUrl":"https://example.org/auth/login","doneUrl":"https://example.org/auth/done"}"#)
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/nonexistent/config");

    let result = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .expect("login succeeds despite missing auth.ini");

    assert_eq!(result, format!("Logged in on {registry}/"));
    let writes = login_writes();
    let token_key = format!("{}:_authToken", nerf_dart(&format!("{registry}/")));
    assert_eq!(written_settings(&writes).get(&token_key), Some("new-token"));
    assert!(
        infos().iter().any(|message| message.contains("https://example.org/auth/login")),
        "got {:?}",
        infos(),
    );
}

#[tokio::test]
async fn should_propagate_non_enoent_errors_from_reading_auth_ini() {
    web_auth_fake!();
    login_fake!(FakeHost, set_ini_read);
    reset();
    reset_login();
    set_fetch(Box::new(|| Ok(ok_token("tok"))));
    set_ini_read(Box::new(|_| Err(io::Error::new(io::ErrorKind::PermissionDenied, "EACCES"))));

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(r#"{"loginUrl":"https://example.org/auth/login","doneUrl":"https://example.org/auth/done"}"#)
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/broken/config");

    let err = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .unwrap_err();

    let LoginError::ReadAuthIni { error, .. } = &err else {
        panic!("expected ReadAuthIni, got {err:?}");
    };
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    // The web-login messages are surfaced before the read is attempted.
    let messages = infos();
    assert_eq!(messages.len(), 2, "expected the auth-URL and Press-ENTER lines: {messages:?}");
    assert!(messages[0].contains("https://example.org/auth/login"), "got {messages:?}");
    assert_eq!(messages[1], "Press ENTER to open the URL in your browser.");
}

/// A web-login probe that fails with a status other than 404 / 405 is fatal:
/// it does not fall back to classic login but surfaces as `WEB_LOGIN_FAILED`,
/// exercising the `Http` arm of `From<WebLoginFlowError>`.
#[tokio::test]
async fn should_surface_a_non_404_web_login_http_error_as_web_login_failed() {
    web_auth_fake!();
    login_fake!(FakeHost);
    reset();
    reset_login();

    let mut server = mockito::Server::new_async().await;
    let login_mock = server
        .mock("POST", "/-/v1/login")
        .with_status(500)
        .with_body("Internal Server Error")
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/mock/config");

    let err = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .unwrap_err();

    login_mock.assert_async().await;
    assert!(matches!(err, LoginError::WebLoginFailed { status: 500, .. }), "got {err:?}");
    assert_eq!(
        miette::Diagnostic::code(&err).map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_WEB_LOGIN_FAILED"),
    );
    assert_eq!(err.to_string(), "Web-based login failed (HTTP 500): Internal Server Error");
}

/// A web-login probe that never reaches the registry surfaces as a transport
/// error (`LoginError::Request`), exercising the `Transport` arm of
/// `From<WebLoginFlowError>`. Binding then dropping an ephemeral loopback
/// socket yields a port that refuses the connection.
#[tokio::test]
async fn should_surface_a_web_login_transport_failure_as_a_request_error() {
    web_auth_fake!();
    login_fake!(FakeHost);
    reset();
    reset_login();

    let addr = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
        listener.local_addr().expect("read the assigned port")
    };
    let registry = format!("http://{addr}/");
    let config_dir = Path::new("/mock/config");

    let err = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .unwrap_err();

    assert!(matches!(err, LoginError::Request { .. }), "got {err:?}");
    assert_eq!(
        miette::Diagnostic::code(&err).map(|code| code.to_string()).as_deref(),
        Some("pacquet_auth_commands::login_request_failed"),
    );
    assert!(err.to_string().starts_with("The login request failed:"), "unexpected message: {err}");
}

/// A `loginUrl` longer than the maximum QR data capacity makes
/// `generate_qr_code` fail before the poll begins, exercising the `QrCode` arm
/// of `From<WebLoginFlowError>`.
#[tokio::test]
async fn should_fail_when_the_login_url_cannot_be_rendered_as_a_qr_code() {
    web_auth_fake!();
    login_fake!(FakeHost);
    reset();
    reset_login();

    let long_login_url = format!("https://example.org/auth/{}", "a".repeat(4000));
    let body = serde_json::json!({
        "loginUrl": long_login_url,
        "doneUrl": "https://example.org/auth/done",
    })
    .to_string();
    let mut server = mockito::Server::new_async().await;
    server.mock("POST", "/-/v1/login").with_status(200).with_body(body).create_async().await;
    let registry = server.url();
    let config_dir = Path::new("/mock/config");

    let err = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .unwrap_err();

    assert!(matches!(err, LoginError::QrCode(_)), "got {err:?}");
    assert_eq!(
        miette::Diagnostic::code(&err).map(|code| code.to_string()).as_deref(),
        Some("pacquet_auth_commands::login_qr_code"),
    );
    assert!(
        err.to_string().starts_with("Failed to render the login QR code:"),
        "unexpected message: {err}",
    );
}

/// When the web-auth poll never sees a token before its budget elapses, the
/// login fails with the transparent web-auth timeout, exercising the `Timeout`
/// arm of `From<WebLoginFlowError>`. Each fake sleep jumps the fake clock past
/// the five-minute budget, so the next poll iteration times out.
#[tokio::test]
async fn should_time_out_when_the_web_auth_poll_never_completes() {
    web_auth_fake!();
    login_fake!(FakeHost);
    reset();
    reset_login();
    set_fetch(Box::new(|| Ok(ok_202())));
    set_sleep_behavior(SleepBehavior::AdvanceByFixed(6 * 60 * 1000));

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(r#"{"loginUrl":"https://example.org/auth/login","doneUrl":"https://example.org/auth/done"}"#)
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/mock/config");

    let err = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .unwrap_err();

    assert!(matches!(err, LoginError::WebAuthTimeout(_)), "got {err:?}");
    assert_eq!(
        miette::Diagnostic::code(&err).map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_WEBAUTH_TIMEOUT"),
    );
    assert_eq!(err.to_string(), "Web-based authentication timed out before it could be completed");
}

/// A `--scope` of a bare `@` is treated as "no scope": the token is stored
/// under the registry key with no scope-to-registry mapping, exercising
/// `normalize_scope`'s empty-scope guard.
#[tokio::test]
async fn should_treat_a_bare_at_scope_as_no_scope() {
    web_auth_fake!();
    login_fake!(FakeHost, login_writes);
    reset();
    reset_login();
    set_fetch(Box::new(|| Ok(ok_token("tok"))));

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(r#"{"loginUrl":"https://example.org/auth/login","doneUrl":"https://example.org/auth/done"}"#)
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/mock/config");

    let mut options = opts(&registry, config_dir);
    options.scope = Some("@");
    login::<FakeHost, RecordingReporter>(&client(), options).await.expect("login");

    let writes = login_writes();
    let settings = written_settings(&writes);
    let config_key = nerf_dart(&format!("{registry}/"));
    assert_eq!(settings.get(&format!("{config_key}:_authToken")), Some("tok"));
    for (_, text) in &writes {
        for line in text.lines() {
            assert!(!line.starts_with('@'), "no scope mapping expected, got line {line:?}");
        }
    }
}

/// A registry-controlled `loginUrl` carrying a control character is never a
/// valid URL; the login is rejected as a possible terminal-spoofing attempt
/// (rather than sanitized and used), and nothing reaches the terminal raw.
#[tokio::test]
async fn rejects_a_login_url_containing_control_characters() {
    web_auth_fake!();
    login_fake!(FakeHost);
    reset();
    reset_login();

    let body = serde_json::json!({
        "loginUrl": "https://example.org/auth/\u{1b}[31mlogin",
        "doneUrl": "https://example.org/auth/done",
    })
    .to_string();
    let mut server = mockito::Server::new_async().await;
    server.mock("POST", "/-/v1/login").with_status(200).with_body(body).create_async().await;
    let registry = server.url();
    let config_dir = Path::new("/mock/config");

    let err = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .unwrap_err();

    assert!(matches!(err, LoginError::UnsafeLoginUrl), "got {err:?}");
    assert_eq!(
        miette::Diagnostic::code(&err).map(|code| code.to_string()).as_deref(),
        Some("pacquet_auth_commands::login_unsafe_url"),
    );
    assert!(infos().iter().all(|message| !message.contains('\u{1b}')), "got {:?}", infos());
}
