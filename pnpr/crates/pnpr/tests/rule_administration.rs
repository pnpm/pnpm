//! The admin API for package rules (`/-/pnpr/v0/admin/rules/{registry}`): what
//! a change does to requests, who may make one, and that changes live in the
//! database.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::{Config, router_with_auth};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::Path,
};
use tempfile::TempDir;
use tower::ServiceExt;

const CONFIG: &str = "\
auth:
  admins: [root]
  users:
    maxUsers: 100
registries:
  local:
    type: hosted
    namespace: \"\"
    teams:
      readers: { name: readers, members: [carol] }
    packages:
      '@secret/*':
        access: [$authenticated]
      '**': {}
  fixed:
    type: hosted
    namespace: fixed
    packages:
      '@fixed/*': {}
  main:
    type: router
    sources: [fixed, local]
defaultRegistry: main
";

const RULES: &str = "/-/pnpr/v0/admin/rules/npm/local";

async fn load_config(dir: &Path) -> Config {
    load_config_with(dir, CONFIG).await
}

async fn load_config_with(dir: &Path, yaml: &str) -> Config {
    let yaml = format!("storage: {}\n{yaml}", storage_dir(dir).display());
    crate::managed_registries::managed_config(&config_path(dir), &yaml, listen(), public_url())
        .await
}

/// The config [`load_config`] wrote, without storing its graph again.
fn reload_config(dir: &Path) -> Config {
    Config::from_yaml(&config_path(dir), listen(), public_url()).unwrap()
}

fn config_path(dir: &Path) -> std::path::PathBuf {
    dir.join("config.yaml")
}

fn storage_dir(dir: &Path) -> std::path::PathBuf {
    let storage = dir.join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    storage
}

fn listen() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873))
}

fn public_url() -> Option<String> {
    Some("http://example.test".to_string())
}

async fn app_router(config: Config) -> axum::Router {
    let auth =
        crate::tokens::seeded_auth(&config, &[("root", "secret123"), ("carol", "secret123")]);
    router_with_auth(config, auth)
}

async fn send_raw(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, axum::body::Bytes) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let body = body.map(|body| Body::from(body.to_string())).unwrap_or_else(Body::empty);
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, bytes)
}

async fn send(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let (status, bytes) = send_raw(app, method, path, token, body).await;
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn add_user(app: &axum::Router, username: &str) -> String {
    let body = json!({ "name": username, "password": "secret123", "type": "user", "roles": [] });
    let path = format!("/-/user/org.couchdb.user:{username}");
    let (status, payload) = send(app, "PUT", &path, None, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED);
    payload["token"]
        .as_str()
        .expect("token in response")
        .to_string()
}

/// The package does not exist, so 404 means the read was allowed and 403
/// that it was refused.
async fn read_secret(app: &axum::Router, token: &str) -> StatusCode {
    send(app, "GET", "/npm/@secret/thing", Some(token), None).await.0
}

#[tokio::test]
async fn a_change_applies_at_once_and_another_change_undoes_it() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let bob = add_user(&app, "bob").await;
    assert_eq!(read_secret(&app, &bob).await, StatusCode::NOT_FOUND);

    let change = json!({ "packages": { "@secret/*": { "access": ["team:readers"] } } });
    let (status, rules) = send(&app, "PUT", RULES, Some(&root), Some(change)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rules["packages"]["@secret/*"]["access"], json!(["team:readers"]));
    assert_eq!(read_secret(&app, &bob).await, StatusCode::FORBIDDEN);
    let carol = add_user(&app, "carol").await;
    assert_eq!(read_secret(&app, &carol).await, StatusCode::NOT_FOUND);

    let undo = json!({ "packages": { "@secret/*": { "access": ["$authenticated"] } } });
    assert_eq!(send(&app, "PUT", RULES, Some(&root), Some(undo)).await.0, StatusCode::OK);
    assert_eq!(read_secret(&app, &bob).await, StatusCode::NOT_FOUND);
    let (_, rules) = send(&app, "GET", RULES, Some(&root), None).await;
    assert_eq!(rules["packages"]["@secret/*"]["access"], json!(["$authenticated"]));
    assert_eq!(rules["version"], json!("3"));
}

#[tokio::test]
async fn changes_must_fit_the_declared_rules() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let refused = [
        json!({ "packages": { "@other/*": { "access": ["$all"] } } }),
        json!({ "packages": { "@secret/*": { "access": ["team:nobody"] } } }),
        json!({ "access": ["$everyone"] }),
        json!({ "unknown": true }),
    ];
    for change in refused {
        let (status, _) = send(&app, "PUT", RULES, Some(&root), Some(change.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{change}");
    }
}

#[tokio::test]
async fn a_pattern_written_another_way_addresses_the_declared_key() {
    let dir = TempDir::new().unwrap();
    let yaml = "\
auth:
  admins: [root]
  users:
    maxUsers: 100
registries:
  wheels:
    type: hosted
    ecosystem: pypi
    packages:
      'Foo_Bar': {}
      '**': {}
";
    let app = app_router(load_config_with(dir.path(), yaml).await).await;
    let root = add_user(&app, "root").await;
    let rules_path = "/-/pnpr/v0/admin/rules/pypi/wheels";
    let (_, rules) = send(&app, "GET", rules_path, Some(&root), None).await;
    assert!(rules["packages"].get("foo-bar").is_some(), "{rules}");

    let change = json!({ "packages": { "foo-bar": { "access": ["root"] } } });
    assert_eq!(send(&app, "PUT", rules_path, Some(&root), Some(change)).await.0, StatusCode::OK);
    let (_, document) = send(&app, "GET", "/-/pnpr/v0/admin/registries", Some(&root), None).await;
    let packages = document["registries"]["wheels"]["packages"].as_object().unwrap();
    assert_eq!(packages.len(), 2, "{packages:?}");
    assert_eq!(packages["Foo_Bar"]["access"], json!(["root"]));
}

#[tokio::test]
async fn only_admins_change_rules() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let bob = add_user(&app, "bob").await;
    let change = json!({ "access": ["$authenticated"] });

    assert_eq!(
        send(&app, "PUT", RULES, Some(&bob), Some(change.clone())).await.0,
        StatusCode::FORBIDDEN,
    );
    assert_eq!(send(&app, "GET", RULES, None, None).await.0, StatusCode::UNAUTHORIZED);
    let fixed = "/-/pnpr/v0/admin/rules/npm/fixed";
    assert_eq!(send(&app, "PUT", fixed, Some(&root), Some(change.clone())).await.0, StatusCode::OK);
    let missing = "/-/pnpr/v0/admin/rules/npm/missing";
    assert_eq!(
        send(&app, "PUT", missing, Some(&root), Some(change)).await.0,
        StatusCode::NOT_FOUND,
    );
    assert_eq!(
        send(&app, "DELETE", RULES, Some(&root), None).await.0,
        StatusCode::METHOD_NOT_ALLOWED,
    );
}

#[tokio::test]
async fn a_new_process_applies_the_stored_changes() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let change = json!({ "packages": { "@secret/*": { "access": ["root"] } } });
    assert_eq!(send(&app, "PUT", RULES, Some(&root), Some(change)).await.0, StatusCode::OK);

    let restarted = app_router(reload_config(dir.path())).await;
    let bob = add_user(&restarted, "bob").await;
    assert_eq!(read_secret(&restarted, &bob).await, StatusCode::FORBIDDEN);
}

/// A rules request carrying `If-Match: "<version>"`. Returns the status and
/// the `ETag` the response carries.
async fn send_if_match(
    app: &axum::Router,
    method: &str,
    token: &str,
    version: &str,
    body: Option<Value>,
) -> (StatusCode, Option<String>) {
    let request = Request::builder()
        .method(method)
        .uri(RULES)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("if-match", format!(r#""{version}""#))
        .body(body.map(|body| Body::from(body.to_string())).unwrap_or_else(Body::empty))
        .unwrap();
    let response = app
        .clone()
        .oneshot(request)
        .await
        .unwrap();
    let etag = response
        .headers()
        .get("etag")
        .map(|etag| {
            etag.to_str()
                .unwrap()
                .trim_matches('"')
                .to_string()
        });
    (response.status(), etag)
}

#[tokio::test]
async fn a_change_based_on_a_stale_version_is_refused() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let (_, rules) = send(&app, "GET", RULES, Some(&root), None).await;
    let loaded = rules["version"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(loaded, "1");

    let mine = json!({ "access": ["$authenticated"] });
    let (status, saved) = send_if_match(&app, "PUT", &root, &loaded, Some(mine)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved.as_deref(), Some("2"));

    let theirs = json!({ "access": ["root"] });
    assert_eq!(
        send_if_match(&app, "PUT", &root, &loaded, Some(theirs)).await.0,
        StatusCode::PRECONDITION_FAILED,
    );
    let (_, rules) = send(&app, "GET", RULES, Some(&root), None).await;
    assert_eq!(rules["access"], json!(["$authenticated"]));
}

#[tokio::test]
async fn a_change_without_if_match_replaces_whatever_is_stored() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let first = json!({ "access": ["$authenticated"] });
    assert_eq!(send(&app, "PUT", RULES, Some(&root), Some(first)).await.0, StatusCode::OK);
    let second = json!({ "access": ["root"] });
    assert_eq!(send(&app, "PUT", RULES, Some(&root), Some(second)).await.0, StatusCode::OK);
    let (_, rules) = send(&app, "GET", RULES, Some(&root), None).await;
    assert_eq!(rules["access"], json!(["root"]));
}

/// A rules request with a raw `If-Match` value. Returns the status.
async fn send_raw_if_match(app: &axum::Router, token: &str, value: &[u8]) -> StatusCode {
    let request = Request::builder()
        .method("PUT")
        .uri(RULES)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("if-match", axum::http::HeaderValue::from_bytes(value).unwrap())
        .body(Body::from(json!({ "access": ["root"] }).to_string()))
        .unwrap();
    app.clone()
        .oneshot(request)
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn if_match_compares_strongly_and_refuses_what_it_cannot_read() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let cases: [(&[u8], StatusCode); 6] = [
        (b"\xff", StatusCode::BAD_REQUEST),
        (b"1", StatusCode::BAD_REQUEST),
        (br#"W/"1""#, StatusCode::PRECONDITION_FAILED),
        (br#""other""#, StatusCode::PRECONDITION_FAILED),
        (br#""other", "1""#, StatusCode::OK),
        (b"*", StatusCode::OK),
    ];
    for (value, expected) in cases {
        let status = send_raw_if_match(&app, &root, value).await;
        assert_eq!(status, expected, "{}", String::from_utf8_lossy(value));
    }
}

#[tokio::test]
async fn a_read_on_another_replica_returns_the_stored_rules_with_their_version() {
    let dir = TempDir::new().unwrap();
    let writer = app_router(load_config(dir.path()).await).await;
    let reader = app_router(reload_config(dir.path())).await;
    let root = add_user(&writer, "root").await;
    let reader_root = add_user(&reader, "root").await;
    let (_, before) = send(&reader, "GET", RULES, Some(&reader_root), None).await;

    let change = json!({ "access": ["$authenticated"] });
    assert_eq!(send(&writer, "PUT", RULES, Some(&root), Some(change)).await.0, StatusCode::OK);
    let (_, after) = send(&reader, "GET", RULES, Some(&reader_root), None).await;
    assert_ne!(after["version"], before["version"]);
    assert_eq!(after["access"], json!(["$authenticated"]));
}

#[tokio::test]
async fn cargo_and_pypi_hosted_registries_reject_unpublish_rules() {
    let dir = TempDir::new().unwrap();
    let yaml = "\
auth:
  admins: [root]
  users:
    maxUsers: 100
registries:
  crates:
    type: hosted
    ecosystem: cargo
    packages:
      'foo': {}
  wheels:
    type: hosted
    ecosystem: pypi
    packages:
      'bar': {}
  main:
    type: router
    sources: [crates]
defaultRegistry: main
";
    let app = app_router(load_config_with(dir.path(), yaml).await).await;
    let root = add_user(&app, "root").await;

    for (path, package, ecosystem) in [
        ("/-/pnpr/v0/admin/rules/cargo/crates", "foo", "cargo"),
        ("/-/pnpr/v0/admin/rules/pypi/wheels", "bar", "pypi"),
    ] {
        let body = json!({ "packages": { package: { "unpublish": ["root"] } } });
        let (status, bytes) = send_raw(&app, "PUT", path, Some(&root), Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let msg = String::from_utf8_lossy(&bytes);
        assert!(
            msg.contains(&format!("serves {ecosystem}, which does not support unpublishing")),
            "expected unpublish error, got: {msg}",
        );
    }
}
