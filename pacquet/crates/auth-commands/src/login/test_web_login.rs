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

use pacquet_network::nerf_dart;
use pacquet_network_web_auth_testing::{ok_token, web_auth_fake};
use pretty_assertions::assert_eq;
use serde_json::json;

use super::{
    login,
    support::{PromptScript, ReadScript, client, login_fake, opts, written_settings},
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
        .with_body(json!({"loginUrl": "https://example.org/auth/login", "doneUrl": "https://example.org/auth/done"}).to_string())
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
