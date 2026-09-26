//! `login` tests for `--scope` handling on the web-login path: the token is
//! keyed to the scope under `_auth`, and the scope is routed to the registry
//! under `registries`.

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
    support::{PromptScript, ReadScript, client, login_fake, opts, written_document},
};

#[tokio::test]
async fn should_persist_a_scoped_auth_token_and_scope_registry_mapping() {
    web_auth_fake!(FakeHost, RecordingReporter, set_fetch);
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
    // The credential and the route that reaches it are one fact: a failure
    // between two writes would persist a token the command reports it failed
    // to record.
    assert_eq!(writes.len(), 1, "the token and its route must land in one write: {writes:?}");
    let document = written_document(&writes);
    let normalized = format!("{registry}/");
    assert_eq!(
        document["_auth"][&normalized],
        json!({ "@my-org": { "authToken": "scoped-token" } }),
    );
    assert_eq!(document["registries"][&normalized], json!({ "scopes": ["@my-org"] }));
}

#[tokio::test]
async fn should_persist_scoped_auth_tokens_under_path_registries() {
    web_auth_fake!(FakeHost, RecordingReporter, set_fetch);
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
    let document = written_document(&login_writes());
    assert_eq!(
        document["_auth"][&registry],
        json!({ "@team": { "authToken": "path-scoped-token" } }),
    );
    assert_eq!(document["registries"][&registry], json!({ "scopes": ["@team"] }));
}

#[tokio::test]
async fn should_accept_scope_with_a_leading_at_and_not_double_prefix() {
    web_auth_fake!(FakeHost, RecordingReporter, set_fetch);
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

    let document = written_document(&login_writes());
    let normalized = format!("{registry}/");
    assert_eq!(document["_auth"][&normalized], json!({ "@my-org": { "authToken": "tok" } }));
    assert_eq!(document["registries"][&normalized], json!({ "scopes": ["@my-org"] }));
}

#[tokio::test]
async fn should_not_write_a_scope_mapping_when_scope_is_omitted() {
    web_auth_fake!(FakeHost, RecordingReporter, set_fetch);
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

    let document = written_document(&login_writes());
    assert_eq!(document["_auth"][format!("{registry}/")], json!({ "@": { "authToken": "tok" } }));
    assert_eq!(document.get("registries"), None);
}

/// A `--scope` of a bare `@` is treated as "no scope": the token is stored
/// under the registry's own scope key with no route recorded, exercising
/// `normalize_scope`'s empty-scope guard.
#[tokio::test]
async fn should_treat_a_bare_at_scope_as_no_scope() {
    web_auth_fake!(FakeHost, RecordingReporter, set_fetch);
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

    let document = written_document(&login_writes());
    assert_eq!(document["_auth"][format!("{registry}/")], json!({ "@": { "authToken": "tok" } }));
    assert_eq!(document.get("registries"), None);
}
