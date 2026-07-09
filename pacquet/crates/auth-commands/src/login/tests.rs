//! Port of `pnpm11/auth/commands/test/login.test.ts`.
//!
//! Each test drives the real [`login`] function. The two registry requests
//! (the web-login `POST` and the classic `PUT`) go through a real
//! [`ThrottledClient`] against a `mockito` server — the "real fixture" route —
//! while the interactive OTP / web-auth effects, the credential prompts, and
//! `auth.ini` I/O are supplied by the `Sys` fake: [`web_auth_fake`] provides the
//! eight web-auth capabilities and [`login_fake`] the four login-specific ones,
//! both over per-test `thread_local!` state so parallel tests never race.

use std::{
    cell::RefCell,
    io,
    path::{Path, PathBuf},
    sync::Mutex,
};

use pacquet_network::{ThrottledClient, nerf_dart};
use pacquet_network_web_auth_testing::{
    InputResponse, SleepBehavior, ok_202, ok_token, web_auth_fake,
};
use pretty_assertions::assert_eq;

use super::{LoginError, LoginOptions, login};
use crate::ini::IniSettings;

/// A scripted response for a credential prompt, keyed on the prompt message.
/// The error half is `dialoguer::Error` — exactly what the real terminal read
/// yields — so a script can drive [`super::prompt_line`]'s real error
/// classification (e.g. an interrupt mapping to a canceled login). `Send`
/// because `prompt_line` calls the fake read from a `spawn_blocking` thread.
type PromptScript = Box<dyn FnMut(&str) -> Result<String, dialoguer::Error> + Send>;

/// A scripted `auth.ini` read.
type ReadScript = Box<dyn FnMut(&Path) -> io::Result<String>>;

/// Expand the login-specific half of the `Sys` fake at the top of a test,
/// after [`web_auth_fake`]. `$fake` is the unit struct `web_auth_fake!`
/// generated (`FakeHost`); this adds the login capabilities to it over fn-local
/// state, plus the `set_*` / `login_writes` / `reset_login` helpers.
///
/// The prompt scripts live in a fn-local `static` [`Mutex`], not `thread_local!`:
/// `prompt_line` runs `prompt_input` / `prompt_password` inside `spawn_blocking`,
/// so they execute on a blocking-pool thread where thread-local state would be
/// invisible. Each test's expansion has its own `static`, so tests stay
/// isolated. `auth.ini` I/O runs on the test thread and stays `thread_local!`.
macro_rules! login_fake {
    ($fake:ident) => {
        static PROMPT_INPUT: Mutex<Option<PromptScript>> = Mutex::new(None);
        static PROMPT_PASSWORD: Mutex<Option<PromptScript>> = Mutex::new(None);
        thread_local! {
            static INI_READ: RefCell<Option<ReadScript>> = const { RefCell::new(None) };
            static INI_WRITES: RefCell<Vec<(PathBuf, String)>> = const { RefCell::new(Vec::new()) };
        }

        impl crate::login::PromptInput for $fake {
            fn prompt_input(message: &str) -> Result<String, dialoguer::Error> {
                let mut script = PROMPT_INPUT.lock().expect("input script mutex");
                (script.as_mut().expect("an input script must be set"))(message)
            }
        }

        impl crate::login::PromptPassword for $fake {
            fn prompt_password(message: &str) -> Result<String, dialoguer::Error> {
                let mut script = PROMPT_PASSWORD.lock().expect("password script mutex");
                (script.as_mut().expect("a password script must be set"))(message)
            }
        }

        impl crate::logout::FsReadToString for $fake {
            fn read_to_string(path: &Path) -> io::Result<String> {
                INI_READ.with(|script| match script.borrow_mut().as_mut() {
                    Some(read) => read(path),
                    None => Ok(String::new()),
                })
            }
        }

        impl crate::logout::FsWrite for $fake {
            fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
                let text = String::from_utf8(bytes.to_vec()).expect("auth.ini is UTF-8");
                INI_WRITES.with(|writes| writes.borrow_mut().push((path.to_path_buf(), text)));
                Ok(())
            }
        }

        #[allow(
            dead_code,
            reason = "the macro emits the full fake surface; a given test drives only the helpers its scenario needs"
        )]
        fn set_prompt_input(script: PromptScript) {
            *PROMPT_INPUT.lock().expect("input script mutex") = Some(script);
        }

        #[allow(
            dead_code,
            reason = "the macro emits the full fake surface; a given test drives only the helpers its scenario needs"
        )]
        fn set_prompt_password(script: PromptScript) {
            *PROMPT_PASSWORD.lock().expect("password script mutex") = Some(script);
        }

        #[allow(
            dead_code,
            reason = "the macro emits the full fake surface; a given test drives only the helpers its scenario needs"
        )]
        fn set_ini_read(script: ReadScript) {
            INI_READ.with(|cell| *cell.borrow_mut() = Some(script));
        }

        #[allow(
            dead_code,
            reason = "the macro emits the full fake surface; a given test drives only the helpers its scenario needs"
        )]
        fn login_writes() -> Vec<(PathBuf, String)> {
            INI_WRITES.with(|writes| writes.borrow().clone())
        }

        #[allow(
            dead_code,
            reason = "the macro emits the full fake surface; a given test drives only the helpers its scenario needs"
        )]
        fn reset_login() {
            *PROMPT_INPUT.lock().expect("input script mutex") = None;
            *PROMPT_PASSWORD.lock().expect("password script mutex") = None;
            INI_READ.with(|cell| *cell.borrow_mut() = None);
            INI_WRITES.with(|writes| writes.borrow_mut().clear());
        }
    };
}

/// A throwaway HTTP client. Requests that reach it target the test's `mockito`
/// server (or, for the pre-network guards, are never sent at all).
fn client() -> ThrottledClient {
    ThrottledClient::default()
}

/// Build [`LoginOptions`] with retry / timeout knobs zeroed — the poll runs
/// against the fake clock, so the real values are irrelevant.
fn opts<'a>(registry: &'a str, config_dir: &'a Path) -> LoginOptions<'a> {
    LoginOptions {
        registry: Some(registry),
        scope: None,
        config_dir,
        fetch_retries: 0,
        fetch_retry_factor: 1,
        fetch_retry_mintimeout: 0,
        fetch_retry_maxtimeout: 0,
        fetch_timeout: 0,
    }
}

/// The `auth.ini` write [`login`] performed, parsed back into [`IniSettings`].
fn written_settings(writes: &[(PathBuf, String)]) -> IniSettings {
    let (_, text) = writes.first().expect("auth.ini was written");
    IniSettings::parse(text)
}

/// The classic-login prompt script the OTP tests share: username / email by
/// message, and a fixed password.
fn credential_prompts(username: &'static str, email: &'static str) -> PromptScript {
    Box::new(move |message| match message {
        "Username:" => Ok(username.to_owned()),
        "Email (this IS public):" => Ok(email.to_owned()),
        other => panic!("unexpected input prompt: {other}"),
    })
}

#[tokio::test]
async fn should_throw_in_non_interactive_terminal() {
    web_auth_fake!();
    login_fake!(FakeHost);
    reset();
    reset_login();
    set_stdin_tty(false);

    let config_dir = Path::new("/mock/config");
    let err =
        login::<FakeHost, RecordingReporter>(&client(), opts("https://example.org", config_dir))
            .await
            .unwrap_err();

    assert!(matches!(err, LoginError::NonInteractive), "got {err:?}");
    assert_eq!(err.to_string(), "The login command requires an interactive terminal");
    assert_eq!(
        miette::Diagnostic::code(&err).map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_LOGIN_NON_INTERACTIVE"),
    );
}

#[tokio::test]
async fn should_use_web_login_when_registry_supports_it() {
    web_auth_fake!();
    login_fake!(FakeHost);
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
    login_fake!(FakeHost);
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
    login_fake!(FakeHost);
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
    login_fake!(FakeHost);
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
    login_fake!(FakeHost);
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
async fn should_fall_back_to_classic_login_when_web_login_returns_404() {
    web_auth_fake!();
    login_fake!(FakeHost);
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
    login_fake!(FakeHost);
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
    login_fake!(FakeHost);
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
    login_fake!(FakeHost);
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
    login_fake!(FakeHost);
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
        miette::Diagnostic::code(&err).map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_LOGIN_FAILED"),
    );
    assert_eq!(err.to_string(), "Login failed (HTTP 403): Forbidden");
}

#[tokio::test]
async fn should_not_trigger_otp_for_401_without_www_authenticate_otp_header() {
    web_auth_fake!();
    login_fake!(FakeHost);
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
        miette::Diagnostic::code(&err).map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_LOGIN_FAILED"),
    );
    assert_eq!(err.to_string(), "Login failed (HTTP 401): Unauthorized");
}

#[tokio::test]
async fn should_throw_when_username_is_empty_in_classic_login() {
    web_auth_fake!();
    login_fake!(FakeHost);
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
        miette::Diagnostic::code(&err).map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_LOGIN_MISSING_CREDENTIALS"),
    );
    assert_eq!(err.to_string(), "Username, password, and email are all required");
}

#[tokio::test]
async fn should_cancel_the_login_when_a_credential_prompt_is_interrupted() {
    web_auth_fake!();
    login_fake!(FakeHost);
    reset();
    reset_login();
    set_prompt_input(credential_prompts("alice", "alice@example.com"));
    // A Ctrl-C at the password prompt surfaces as dialoguer's interrupted I/O
    // error; `prompt_line`'s real classification must turn it into a canceled
    // login. The fake returns the raw `dialoguer::Error`, so this exercises the
    // wrapper rather than short-circuiting it with a pre-mapped `PromptError`.
    set_prompt_password(Box::new(|_| {
        Err(dialoguer::Error::IO(io::Error::from(io::ErrorKind::Interrupted)))
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
        miette::Diagnostic::code(&err).map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_LOGIN_CANCELED"),
    );
    assert_eq!(err.to_string(), "Login canceled");
}

#[tokio::test]
async fn should_throw_when_classic_login_returns_no_token() {
    web_auth_fake!();
    login_fake!(FakeHost);
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
        miette::Diagnostic::code(&err).map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_LOGIN_NO_TOKEN"),
    );
    assert_eq!(err.to_string(), "The registry did not return an authentication token");
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
    login_fake!(FakeHost);
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
    login_fake!(FakeHost);
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

/// A credential prompt that fails with a non-interrupt I/O error surfaces as
/// `LoginError::Prompt` rather than a cancellation, exercising `prompt_line`'s
/// `PromptError::Other` classification and `read_credential`'s catch-all arm.
#[tokio::test]
async fn should_surface_a_non_interrupt_prompt_failure_as_a_prompt_error() {
    web_auth_fake!();
    login_fake!(FakeHost);
    reset();
    reset_login();
    set_prompt_input(Box::new(|_| {
        Err(dialoguer::Error::IO(io::Error::from(io::ErrorKind::BrokenPipe)))
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
        miette::Diagnostic::code(&err).map(|code| code.to_string()).as_deref(),
        Some("pacquet_auth_commands::login_prompt_failed"),
    );
    assert!(
        err.to_string().starts_with("Failed to read the login prompt:"),
        "unexpected message: {err}",
    );
}

/// A `--scope` of a bare `@` is treated as "no scope": the token is stored
/// under the registry key with no scope-to-registry mapping, exercising
/// `normalize_scope`'s empty-scope guard.
#[tokio::test]
async fn should_treat_a_bare_at_scope_as_no_scope() {
    web_auth_fake!();
    login_fake!(FakeHost);
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
