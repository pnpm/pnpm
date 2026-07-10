//! `login` tests for `--scope` handling on the web-login path: the token is
//! keyed to the scope and a scope-to-registry mapping is recorded.

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
        .with_body(json!({"loginUrl": "https://my-org.example/auth/login", "doneUrl": "https://my-org.example/auth/done"}).to_string())
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
        .with_body(json!({"loginUrl": "https://example.com/auth/login", "doneUrl": "https://example.com/auth/done"}).to_string())
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
        .with_body(json!({"loginUrl": "https://my-org.example/auth/login", "doneUrl": "https://my-org.example/auth/done"}).to_string())
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
        .with_body(json!({"loginUrl": "https://example.com/auth/login", "doneUrl": "https://example.com/auth/done"}).to_string())
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
        .with_body(json!({"loginUrl": "https://example.org/auth/login", "doneUrl": "https://example.org/auth/done"}).to_string())
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
