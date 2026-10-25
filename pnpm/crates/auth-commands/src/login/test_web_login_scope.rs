//! `login` tests for `--scope` handling on the web-login path: the token is
//! keyed to the scope in `auth.ini`, and the scope is routed to the registry
//! under `registries` in the global `config.yaml`.

use std::{
    cell::RefCell,
    io,
    path::{Path, PathBuf},
    sync::Mutex,
};

use pnpm_network::nerf_dart;
use pnpm_network_web_auth_testing::{ok_token, web_auth_fake};
use pretty_assertions::assert_eq;
use serde_json::json;

use super::{
    login,
    support::{
        PromptScript, ReadScript, client, login_fake, opts, written_document, written_settings,
    },
};

/// The token `writes` recorded in `auth.ini` for `scope` at `registry`.
fn scoped_token(writes: &[(PathBuf, String)], registry: &str, scope: &str) -> Option<String> {
    written_settings(writes)
        .get(&format!("{}:{scope}:_authToken", nerf_dart(registry)))
        .map(str::to_owned)
}

/// Whether `writes` touched the global `config.yaml`.
fn wrote_config_yaml(writes: &[(PathBuf, String)]) -> bool {
    writes
        .iter()
        .any(|(path, _)| path.ends_with("config.yaml"))
}

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
    let normalized = format!("{registry}/");
    assert_eq!(scoped_token(&writes, &normalized, "@my-org").as_deref(), Some("scoped-token"));
    let document = written_document(&writes);
    assert_eq!(document.get("_auth"), None);
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
    let writes = login_writes();
    assert_eq!(scoped_token(&writes, &registry, "@team").as_deref(), Some("path-scoped-token"));
    assert_eq!(written_document(&writes)["registries"][&registry], json!({ "scopes": ["@team"] }));
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

    let writes = login_writes();
    let normalized = format!("{registry}/");
    assert_eq!(scoped_token(&writes, &normalized, "@my-org").as_deref(), Some("tok"));
    assert_eq!(scoped_token(&writes, &normalized, "@@my-org"), None);
    assert_eq!(
        written_document(&writes)["registries"][&normalized],
        json!({ "scopes": ["@my-org"] }),
    );
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

    let writes = login_writes();
    let token_key = format!("{}:_authToken", nerf_dart(&format!("{registry}/")));
    assert_eq!(written_settings(&writes).get(&token_key), Some("tok"));
    assert!(!wrote_config_yaml(&writes), "an unscoped login records no route: {writes:?}");
}

/// A `--scope` of a bare `@` is treated as "no scope": the token is stored
/// under the registry's own key with no route recorded, exercising
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

    let writes = login_writes();
    let token_key = format!("{}:_authToken", nerf_dart(&format!("{registry}/")));
    assert_eq!(written_settings(&writes).get(&token_key), Some("tok"));
    assert!(!wrote_config_yaml(&writes), "an unscoped login records no route: {writes:?}");
}

/// `_auth` outranks `auth.ini`, so a token an earlier version recorded there
/// for the same scope would shadow the new one if the login left it behind.
#[tokio::test]
async fn should_drop_the_token_an_earlier_login_recorded_in_config_yaml() {
    web_auth_fake!(FakeHost, RecordingReporter, set_fetch);
    login_fake!(FakeHost, set_ini_read, login_writes);
    reset();
    reset_login();
    set_fetch(Box::new(|| Ok(ok_token("new-token"))));

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(json!({"loginUrl": "https://example.org/auth/login", "doneUrl": "https://example.org/auth/done"}).to_string())
        .create_async()
        .await;
    let registry = server.url();
    let normalized = format!("{registry}/");
    let stale = format!(
        "_auth:\n  {normalized}:\n    '@my-org': {{ authToken: stale-token }}\n    '@': {{ authToken: own-token }}\n",
    );
    set_ini_read(Box::new(move |path| {
        Ok(if path.ends_with("config.yaml") { stale.clone() } else { String::new() })
    }));

    let mut options = opts(&registry, Path::new("/mock/config"));
    options.scope = Some("@my-org");
    login::<FakeHost, RecordingReporter>(&client(), options).await.expect("scoped login");

    let writes = login_writes();
    assert_eq!(scoped_token(&writes, &normalized, "@my-org").as_deref(), Some("new-token"));
    assert_eq!(
        written_document(&writes)["_auth"],
        json!({ normalized.clone(): { "@": { "authToken": "own-token" } } }),
    );
}
