//! `login` tests for the web-login happy path: the registry `POST` returns 200
//! with a usable `loginUrl` / `doneUrl`, so login completes over the web flow
//! without falling back to the classic `PUT`. Scope handling lives in
//! `test_web_login_scope`; the error paths in `test_web_login_errors`.

use std::{
    cell::RefCell,
    io,
    path::{Path, PathBuf},
    sync::Mutex,
};

use pnpm_network_web_auth_testing::{ok_token, web_auth_fake};
use pretty_assertions::assert_eq;
use serde_json::json;

use super::{
    login,
    support::{PromptScript, ReadScript, client, login_fake, opts, written_registry_token},
};

#[tokio::test]
async fn should_use_web_login_when_registry_supports_it() {
    web_auth_fake!(FakeHost, RecordingReporter, set_fetch, infos);
    login_fake!(FakeHost, login_writes);
    reset();
    reset_login();
    set_fetch(Box::new(|| Ok(ok_token("web-auth-token-123"))));

    let mut server = mockito::Server::new_async().await;
    let login_mock = server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(json!({"loginUrl": "https://example.com/auth/login", "doneUrl": "https://example.com/auth/done"}).to_string())
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
    let (path, _) = writes.first().expect("config.yaml was written");
    assert_eq!(path, &config_dir.join("config.yaml"));
    assert_eq!(
        written_registry_token(&writes, &format!("{registry}/")),
        Some("web-auth-token-123".to_owned()),
    );

    let messages = infos();
    assert_eq!(messages.len(), 2, "expected the auth-URL and Press-ENTER lines: {messages:?}");
    assert!(messages[0].contains("https://example.com/auth/login"), "got {messages:?}");
    assert_eq!(messages[1], "Press ENTER to open the URL in your browser.");
}

/// The registry can deliver display-only text such as a login verification
/// code through the `npm-notice` response header; every value is printed
/// ahead of the auth-URL and Press-ENTER lines, in the order received.
#[tokio::test]
async fn should_print_the_npm_notice_headers_before_the_auth_url() {
    web_auth_fake!(FakeHost, RecordingReporter, set_fetch, infos);
    login_fake!(FakeHost);
    reset();
    reset_login();
    set_fetch(Box::new(|| Ok(ok_token("web-auth-token-123"))));

    let mut server = mockito::Server::new_async().await;
    let login_mock = server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_header("npm-notice", "Verification code: 123456. Enter this code in the browser to complete the login.")
        .with_header("npm-notice", "The code expires in five minutes.")
        .with_body(json!({"loginUrl": "https://example.com/auth/login", "doneUrl": "https://example.com/auth/done"}).to_string())
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/custom/config");

    let result = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .expect("web login succeeds");

    login_mock.assert_async().await;
    assert_eq!(result, format!("Logged in on {registry}/"));

    let messages = infos();
    assert_eq!(
        messages.len(),
        4,
        "expected the notices, auth-URL, and Press-ENTER lines: {messages:?}"
    );
    assert_eq!(
        messages[0],
        "Verification code: 123456. Enter this code in the browser to complete the login."
    );
    assert_eq!(messages[1], "The code expires in five minutes.");
    assert!(messages[2].contains("https://example.com/auth/login"), "got {messages:?}");
    assert_eq!(messages[3], "Press ENTER to open the URL in your browser.");
}

/// A notice is registry-controlled text, so control characters are removed
/// before it reaches the terminal. HTTP header values cannot carry C0
/// controls like ESC or CR/LF, but they can carry a tab and UTF-8-encoded
/// C1 controls such as CSI (U+009B) and NEL (U+0085); the spoofed escape
/// still must not survive.
#[tokio::test]
async fn should_strip_control_characters_from_an_npm_notice() {
    web_auth_fake!(FakeHost, RecordingReporter, set_fetch, infos);
    login_fake!(FakeHost);
    reset();
    reset_login();
    set_fetch(Box::new(|| Ok(ok_token("web-auth-token-123"))));

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_header("npm-notice", "\u{9b}31mVerification code:\t123456\u{85}")
        .with_body(json!({"loginUrl": "https://example.com/auth/login", "doneUrl": "https://example.com/auth/done"}).to_string())
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/custom/config");

    login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .expect("web login succeeds");

    let messages = infos();
    assert_eq!(messages[0], "31mVerification code:123456");
    assert!(!messages[0].contains(char::is_control), "got {messages:?}");
}

/// A notice that holds only control characters is empty once sanitized, so
/// nothing is emitted for it.
#[tokio::test]
async fn should_skip_an_npm_notice_that_is_empty_after_sanitizing() {
    web_auth_fake!(FakeHost, RecordingReporter, set_fetch, infos);
    login_fake!(FakeHost);
    reset();
    reset_login();
    set_fetch(Box::new(|| Ok(ok_token("web-auth-token-123"))));

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_header("npm-notice", "\u{85}\t\u{9b}")
        .with_body(json!({"loginUrl": "https://example.com/auth/login", "doneUrl": "https://example.com/auth/done"}).to_string())
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/custom/config");

    login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .expect("web login succeeds");

    let messages = infos();
    assert_eq!(messages.len(), 2, "expected the auth-URL and Press-ENTER lines: {messages:?}");
    assert!(messages[0].contains("https://example.com/auth/login"), "got {messages:?}");
    assert_eq!(messages[1], "Press ENTER to open the URL in your browser.");
}

#[tokio::test]
async fn should_complete_web_login_without_an_interactive_terminal() {
    web_auth_fake!(FakeHost, RecordingReporter, set_stdin_tty, set_stdout_tty, set_fetch, infos);
    login_fake!(FakeHost, login_writes);
    reset();
    reset_login();
    set_stdin_tty(false);
    set_stdout_tty(false);
    set_fetch(Box::new(|| Ok(ok_token("headless-token"))));

    let mut server = mockito::Server::new_async().await;
    let login_mock = server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(json!({"loginUrl": "https://example.com/auth/login", "doneUrl": "https://example.com/auth/done"}).to_string())
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/mock/config");

    let result = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .expect("web login succeeds without a TTY");

    login_mock.assert_async().await;
    assert_eq!(result, format!("Logged in on {registry}/"));

    let writes = login_writes();
    assert_eq!(
        written_registry_token(&writes, &format!("{registry}/")),
        Some("headless-token".to_owned()),
    );

    // No QR code (stdout is not a terminal) and no "Press ENTER" prompt.
    assert_eq!(infos(), ["Authenticate your account at:\nhttps://example.com/auth/login"]);
}

#[tokio::test]
async fn should_log_in_to_a_registry_under_a_subpath_without_a_trailing_slash() {
    web_auth_fake!(FakeHost, RecordingReporter, set_fetch);
    login_fake!(FakeHost, login_writes);
    reset();
    reset_login();
    set_fetch(Box::new(|| Ok(ok_token("subpath-token"))));

    let mut server = mockito::Server::new_async().await;
    let login_mock = server
        .mock("POST", "/npm/registry/-/v1/login")
        .with_status(200)
        .with_body(json!({"loginUrl": "https://example.com/auth/login", "doneUrl": "https://example.com/auth/done"}).to_string())
        .create_async()
        .await;
    let registry = format!("{}/npm/registry", server.url());
    let config_dir = Path::new("/mock/config");

    let result = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .expect("web login succeeds on a subpath registry");

    login_mock.assert_async().await;
    assert_eq!(result, format!("Logged in on {registry}/"));

    let writes = login_writes();
    assert_eq!(
        written_registry_token(&writes, &format!("{registry}/")),
        Some("subpath-token".to_owned()),
    );
}

#[tokio::test]
async fn should_succeed_when_config_file_does_not_exist() {
    web_auth_fake!(FakeHost, RecordingReporter, set_fetch, infos);
    login_fake!(FakeHost, set_ini_read, login_writes);
    reset();
    reset_login();
    set_fetch(Box::new(|| Ok(ok_token("new-token"))));
    set_ini_read(Box::new(|_| Err(io::Error::new(io::ErrorKind::NotFound, "ENOENT"))));

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(json!({"loginUrl": "https://example.org/auth/login", "doneUrl": "https://example.org/auth/done"}).to_string())
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/nonexistent/config");

    let result = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .expect("login succeeds despite a missing config.yaml");

    assert_eq!(result, format!("Logged in on {registry}/"));
    let writes = login_writes();
    assert_eq!(
        written_registry_token(&writes, &format!("{registry}/")),
        Some("new-token".to_owned()),
    );
    assert!(
        infos()
            .iter()
            .any(|message| message.contains("https://example.org/auth/login")),
        "got {:?}",
        infos(),
    );
}
