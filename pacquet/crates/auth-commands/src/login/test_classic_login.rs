//! `login` tests for the classic fallback: the registry `POST` returns 404/405,
//! so login falls back to the classic `PUT` credential flow (with OTP handling).

use std::{
    cell::RefCell,
    io,
    path::{Path, PathBuf},
    sync::Mutex,
};

use pacquet_network::nerf_dart;
use pacquet_network_web_auth_testing::{InputResponse, ok_token, web_auth_fake};
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;

use super::{
    LoginError, login,
    test_support::{
        PromptScript, ReadScript, client, credential_prompts, login_fake, opts, written_settings,
    },
};

#[tokio::test]
async fn should_fall_back_to_classic_login_when_web_login_returns_404() {
    web_auth_fake!();
    login_fake!(FakeHost, set_prompt_input, set_prompt_password, login_writes);
    reset();
    reset_login();
    set_prompt_input(credential_prompts("john", "john@example.com"));
    set_prompt_password(Box::new(|_| Ok("secret".to_owned())));

    let mut server = mockito::Server::new_async().await;
    let login_mock = server
        .mock("POST", "/-/v1/login")
        .with_status(404)
        .with_body("Not Found")
        .create_async()
        .await;
    let add_user_mock = server
        .mock("PUT", "/-/user/org.couchdb.user:john")
        .with_status(201)
        .with_body(r#"{"ok":true,"token":"classic-token-456"}"#)
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/other/config");

    let result = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .expect("classic login succeeds");

    login_mock.assert_async().await;
    add_user_mock.assert_async().await;
    assert_eq!(result, format!("Logged in on {registry}/"));

    let writes = login_writes();
    let (path, _) = writes.first().expect("auth.ini was written");
    assert_eq!(path, &config_dir.join("auth.ini"));
    let token_key = format!("{}:_authToken", nerf_dart(&format!("{registry}/")));
    assert_eq!(written_settings(&writes).get(&token_key), Some("classic-token-456"));
    assert_eq!(infos(), ["Logged in as john"]);
}

#[tokio::test]
async fn should_fall_back_to_classic_login_when_web_login_returns_405() {
    web_auth_fake!();
    login_fake!(FakeHost, set_prompt_input, set_prompt_password, login_writes);
    reset();
    reset_login();
    set_prompt_input(credential_prompts("jane", "jane@example.com"));
    set_prompt_password(Box::new(|_| Ok("pass".to_owned())));

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/-/v1/login")
        .with_status(405)
        .with_body("Method Not Allowed")
        .create_async()
        .await;
    server
        .mock("PUT", "/-/user/org.couchdb.user:jane")
        .with_status(201)
        .with_body(r#"{"ok":true,"token":"token-405"}"#)
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/mock/config");

    let result = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .expect("classic login succeeds");

    assert_eq!(result, format!("Logged in on {registry}/"));
    let writes = login_writes();
    let token_key = format!("{}:_authToken", nerf_dart(&format!("{registry}/")));
    assert_eq!(written_settings(&writes).get(&token_key), Some("token-405"));
    assert_eq!(infos(), ["Logged in as jane"]);
}

#[tokio::test]
async fn should_handle_classic_otp_challenge_during_login() {
    web_auth_fake!();
    login_fake!(FakeHost, set_prompt_input, set_prompt_password);
    reset();
    reset_login();
    set_prompt_input(credential_prompts("alice", "alice@example.com"));
    set_prompt_password(Box::new(|_| Ok("pass".to_owned())));
    set_input(InputResponse::Value(Some("999999".to_owned())));

    let mut server = mockito::Server::new_async().await;
    server.mock("POST", "/-/v1/login").with_status(404).with_body("Not Found").create_async().await;
    let challenge = server
        .mock("PUT", "/-/user/org.couchdb.user:alice")
        .match_header("npm-otp", mockito::Matcher::Missing)
        .with_status(401)
        .with_header("www-authenticate", "OTP otp")
        .with_body(r#"{"error":"otp required"}"#)
        .expect(1)
        .create_async()
        .await;
    let retry = server
        .mock("PUT", "/-/user/org.couchdb.user:alice")
        .match_header("npm-otp", "999999")
        .with_status(201)
        .with_body(r#"{"ok":true,"token":"otp-token-789"}"#)
        .expect(1)
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/otp/config");

    let result = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .expect("classic OTP login succeeds");

    challenge.assert_async().await;
    retry.assert_async().await;
    assert_eq!(result, format!("Logged in on {registry}/"));
    assert_eq!(infos(), ["Logged in as alice"]);
}

#[tokio::test]
async fn should_handle_webauth_otp_challenge_during_login() {
    web_auth_fake!();
    login_fake!(FakeHost, set_prompt_input, set_prompt_password);
    reset();
    reset_login();
    set_prompt_input(credential_prompts("bob", "bob@example.com"));
    set_prompt_password(Box::new(|_| Ok("pass".to_owned())));
    set_fetch(Box::new(|| Ok(ok_token("web-tok"))));

    let mut server = mockito::Server::new_async().await;
    server.mock("POST", "/-/v1/login").with_status(404).with_body("Not Found").create_async().await;
    let challenge = server
        .mock("PUT", "/-/user/org.couchdb.user:bob")
        .match_header("npm-otp", mockito::Matcher::Missing)
        .with_status(401)
        .with_header("www-authenticate", "OTP otp")
        .with_body(r#"{"authUrl":"https://example.org/auth/web","doneUrl":"https://example.org/auth/web/done"}"#)
        .expect(1)
        .create_async()
        .await;
    let retry = server
        .mock("PUT", "/-/user/org.couchdb.user:bob")
        .match_header("npm-otp", "web-tok")
        .with_status(201)
        .with_body(r#"{"ok":true,"token":"final-token"}"#)
        .expect(1)
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/otp/config");

    let result = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .expect("web-auth OTP login succeeds");

    challenge.assert_async().await;
    retry.assert_async().await;
    assert_eq!(result, format!("Logged in on {registry}/"));
    assert!(
        infos().iter().any(|message| message.contains("https://example.org/auth/web")),
        "the auth URL should be surfaced, got {:?}",
        infos(),
    );
}

#[tokio::test]
async fn should_not_trigger_otp_for_non_401_errors() {
    web_auth_fake!();
    login_fake!(FakeHost, set_prompt_input, set_prompt_password);
    reset();
    reset_login();
    set_prompt_input(credential_prompts("alice", "alice@example.com"));
    set_prompt_password(Box::new(|_| Ok("pass".to_owned())));

    let mut server = mockito::Server::new_async().await;
    server.mock("POST", "/-/v1/login").with_status(404).with_body("Not Found").create_async().await;
    server
        .mock("PUT", "/-/user/org.couchdb.user:alice")
        .with_status(403)
        .with_body("Forbidden")
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/mock/config");

    let err = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .unwrap_err();

    assert_eq!(
        err.pipe_ref(miette::Diagnostic::code).map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_LOGIN_FAILED"),
    );
    assert_eq!(err.to_string(), "Login failed (HTTP 403): Forbidden");
}

#[tokio::test]
async fn should_not_trigger_otp_for_401_without_www_authenticate_otp_header() {
    web_auth_fake!();
    login_fake!(FakeHost, set_prompt_input, set_prompt_password);
    reset();
    reset_login();
    set_prompt_input(credential_prompts("alice", "alice@example.com"));
    set_prompt_password(Box::new(|_| Ok("pass".to_owned())));

    let mut server = mockito::Server::new_async().await;
    server.mock("POST", "/-/v1/login").with_status(404).with_body("Not Found").create_async().await;
    server
        .mock("PUT", "/-/user/org.couchdb.user:alice")
        .with_status(401)
        .with_body("Unauthorized")
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/otp/config");

    let err = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .unwrap_err();

    assert_eq!(
        err.pipe_ref(miette::Diagnostic::code).map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_LOGIN_FAILED"),
    );
    assert_eq!(err.to_string(), "Login failed (HTTP 401): Unauthorized");
}

#[tokio::test]
async fn should_throw_when_username_is_empty_in_classic_login() {
    web_auth_fake!();
    login_fake!(FakeHost, set_prompt_input, set_prompt_password);
    reset();
    reset_login();
    set_prompt_input(Box::new(|message| match message {
        "Username:" => Ok(String::new()),
        "Email (this IS public):" => Ok("a@b.com".to_owned()),
        other => panic!("unexpected input prompt: {other}"),
    }));
    set_prompt_password(Box::new(|_| Ok("pass".to_owned())));

    let mut server = mockito::Server::new_async().await;
    server.mock("POST", "/-/v1/login").with_status(404).with_body("Not Found").create_async().await;
    let registry = server.url();
    let config_dir = Path::new("/mock/config");

    let err = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .unwrap_err();

    assert!(matches!(err, LoginError::MissingCredentials), "got {err:?}");
    assert_eq!(
        err.pipe_ref(miette::Diagnostic::code).map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_LOGIN_MISSING_CREDENTIALS"),
    );
    assert_eq!(err.to_string(), "Username, password, and email are all required");
}

#[tokio::test]
async fn should_cancel_the_login_when_a_credential_prompt_is_interrupted() {
    web_auth_fake!();
    login_fake!(FakeHost, set_prompt_input, set_prompt_password);
    reset();
    reset_login();
    set_prompt_input(credential_prompts("alice", "alice@example.com"));
    // A Ctrl-C at the password prompt surfaces as dialoguer's interrupted I/O
    // error; `prompt_line`'s real classification must turn it into a canceled
    // login. The fake returns the raw `dialoguer::Error`, so this exercises the
    // wrapper rather than short-circuiting it with a pre-mapped `PromptError`.
    set_prompt_password(Box::new(|_| {
        io::ErrorKind::Interrupted.pipe(io::Error::from).pipe(dialoguer::Error::IO).pipe(Err)
    }));

    let mut server = mockito::Server::new_async().await;
    server.mock("POST", "/-/v1/login").with_status(404).with_body("Not Found").create_async().await;
    let registry = server.url();
    let config_dir = Path::new("/mock/config");

    let err = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .unwrap_err();

    assert!(matches!(err, LoginError::Canceled), "got {err:?}");
    assert_eq!(
        err.pipe_ref(miette::Diagnostic::code).map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_LOGIN_CANCELED"),
    );
    assert_eq!(err.to_string(), "Login canceled");
}

#[tokio::test]
async fn should_throw_when_classic_login_returns_no_token() {
    web_auth_fake!();
    login_fake!(FakeHost, set_prompt_input, set_prompt_password);
    reset();
    reset_login();
    set_prompt_input(credential_prompts("alice", "alice@example.com"));
    set_prompt_password(Box::new(|_| Ok("pass".to_owned())));

    let mut server = mockito::Server::new_async().await;
    server.mock("POST", "/-/v1/login").with_status(404).with_body("Not Found").create_async().await;
    server
        .mock("PUT", "/-/user/org.couchdb.user:alice")
        .with_status(201)
        .with_body(r#"{"ok":true}"#)
        .create_async()
        .await;
    let registry = server.url();
    let config_dir = Path::new("/mock/config");

    let err = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .unwrap_err();

    assert_eq!(
        err.pipe_ref(miette::Diagnostic::code).map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_LOGIN_NO_TOKEN"),
    );
    assert_eq!(err.to_string(), "The registry did not return an authentication token");
}

/// A credential prompt that fails with a non-interrupt I/O error surfaces as
/// `LoginError::Prompt` rather than a cancellation, exercising `prompt_line`'s
/// `PromptError::Other` classification and `read_credential`'s catch-all arm.
#[tokio::test]
async fn should_surface_a_non_interrupt_prompt_failure_as_a_prompt_error() {
    web_auth_fake!();
    login_fake!(FakeHost, set_prompt_input);
    reset();
    reset_login();
    set_prompt_input(Box::new(|_| {
        io::ErrorKind::BrokenPipe.pipe(io::Error::from).pipe(dialoguer::Error::IO).pipe(Err)
    }));

    let mut server = mockito::Server::new_async().await;
    server.mock("POST", "/-/v1/login").with_status(404).with_body("Not Found").create_async().await;
    let registry = server.url();
    let config_dir = Path::new("/mock/config");

    let err = login::<FakeHost, RecordingReporter>(&client(), opts(&registry, config_dir))
        .await
        .unwrap_err();

    assert!(matches!(err, LoginError::Prompt { .. }), "got {err:?}");
    assert_eq!(
        err.pipe_ref(miette::Diagnostic::code).map(|code| code.to_string()).as_deref(),
        Some("pacquet_auth_commands::login_prompt_failed"),
    );
    assert!(
        err.to_string().starts_with("Failed to read the login prompt:"),
        "unexpected message: {err}",
    );
}
