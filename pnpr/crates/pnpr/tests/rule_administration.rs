//! The admin API for package rules (`/-/pnpr/v0/admin/rules/{registry}`) on a
//! registry with `rulesManagedBy: api`: what a change does to requests, who
//! may make one, and that changes live in the hosted store.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::{AuthState, Config, router_with_auth};
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
  htpasswd:
    max_users: 100
registries:
  local:
    type: hosted
    org: \"\"
    rulesManagedBy: api
    teams:
      readers: [carol]
    packages:
      '@secret/*':
        access: [$authenticated]
      '**': {}
  fixed:
    type: hosted
    org: fixed
    packages:
      '@fixed/*': {}
  main:
    type: router
    sources: [fixed, local]
defaultRegistry: main
";

const RULES: &str = "/-/pnpr/v0/admin/rules/npm/local";

fn load_config(dir: &Path) -> Config {
    let storage = dir.join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    let path = dir.join("config.yaml");
    std::fs::write(&path, format!("storage: {}\n{CONFIG}", storage.display())).unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    Config::from_yaml(&path, listen, Some("http://example.test".to_string())).unwrap()
}

async fn app_router(config: Config) -> axum::Router {
    let auth = AuthState::in_memory_with_max_users(config.identity.auth.htpasswd.max_users);
    auth.users.create_user("root", "secret").await.unwrap();
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
    let body = json!({ "name": username, "password": "secret", "type": "user", "roles": [] });
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
async fn a_change_applies_at_once_and_a_reset_undoes_it() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path())).await;
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

    assert_eq!(send(&app, "DELETE", RULES, Some(&root), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(read_secret(&app, &bob).await, StatusCode::NOT_FOUND);
    let (_, rules) = send(&app, "GET", RULES, Some(&root), None).await;
    assert_eq!(rules["packages"]["@secret/*"]["access"], json!(["$authenticated"]));
    assert_eq!(rules["rulesManagedBy"], json!("api"));
}

#[tokio::test]
async fn changes_must_fit_the_declared_rules() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path())).await;
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
async fn only_admins_change_rules_and_only_where_the_api_manages_them() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path())).await;
    let root = add_user(&app, "root").await;
    let bob = add_user(&app, "bob").await;
    let change = json!({ "access": ["$authenticated"] });

    assert_eq!(
        send(&app, "PUT", RULES, Some(&bob), Some(change.clone())).await.0,
        StatusCode::FORBIDDEN,
    );
    assert_eq!(send(&app, "GET", RULES, None, None).await.0, StatusCode::UNAUTHORIZED);
    let fixed = "/-/pnpr/v0/admin/rules/npm/fixed";
    assert_eq!(
        send(&app, "PUT", fixed, Some(&root), Some(change.clone())).await.0,
        StatusCode::FORBIDDEN,
    );
    let (status, rules) = send(&app, "GET", fixed, Some(&root), None).await;
    assert_eq!((status, &rules["rulesManagedBy"]), (StatusCode::OK, &json!("config")));
    let missing = "/-/pnpr/v0/admin/rules/npm/missing";
    assert_eq!(
        send(&app, "PUT", missing, Some(&root), Some(change)).await.0,
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn a_new_process_applies_the_stored_changes() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path())).await;
    let root = add_user(&app, "root").await;
    let change = json!({ "packages": { "@secret/*": { "access": ["root"] } } });
    assert_eq!(send(&app, "PUT", RULES, Some(&root), Some(change)).await.0, StatusCode::OK);

    let restarted = app_router(load_config(dir.path())).await;
    let bob = add_user(&restarted, "bob").await;
    assert_eq!(read_secret(&restarted, &bob).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn unreadable_stored_changes_close_the_registry() {
    let dir = TempDir::new().unwrap();
    let record = dir.path().join("storage/.rule-overrides/v0/local.json");
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    std::fs::write(&record, "not rules").unwrap();
    let app = app_router(load_config(dir.path())).await;
    let bob = add_user(&app, "bob").await;

    let (_, directory) = send(&app, "GET", "/-/pnpr/v0/registries", Some(&bob), None).await;
    let names: Vec<&str> = directory["registries"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["name"].as_str())
        .collect();
    assert!(!names.contains(&"local"), "{names:?}");
    assert!(names.contains(&"fixed"), "{names:?}");
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
    let app = app_router(load_config(dir.path())).await;
    let root = add_user(&app, "root").await;
    let (_, rules) = send(&app, "GET", RULES, Some(&root), None).await;
    let loaded = rules["version"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(loaded, "config");

    let mine = json!({ "access": ["$authenticated"] });
    let (status, saved) = send_if_match(&app, "PUT", &root, &loaded, Some(mine.clone())).await;
    assert_eq!(status, StatusCode::OK);
    let saved = saved.unwrap();
    assert_ne!(saved, loaded);

    assert_eq!(
        send_if_match(&app, "PUT", &root, &loaded, Some(mine)).await.0,
        StatusCode::PRECONDITION_FAILED,
    );
    assert_eq!(
        send_if_match(&app, "DELETE", &root, &loaded, None).await.0,
        StatusCode::PRECONDITION_FAILED,
    );
    let (_, rules) = send(&app, "GET", RULES, Some(&root), None).await;
    assert_eq!(rules["access"], json!(["$authenticated"]));

    assert_eq!(send_if_match(&app, "DELETE", &root, &saved, None).await.0, StatusCode::NO_CONTENT);
    let (_, rules) = send(&app, "GET", RULES, Some(&root), None).await;
    assert_eq!(rules["access"], json!(["$all"]));
}

#[tokio::test]
async fn a_change_without_if_match_replaces_whatever_is_stored() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path())).await;
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
    let app = app_router(load_config(dir.path())).await;
    let root = add_user(&app, "root").await;
    let cases: [(&[u8], StatusCode); 6] = [
        (b"\xff", StatusCode::BAD_REQUEST),
        (b"config", StatusCode::BAD_REQUEST),
        (br#"W/"config""#, StatusCode::PRECONDITION_FAILED),
        (br#""other""#, StatusCode::PRECONDITION_FAILED),
        (br#""other", "config""#, StatusCode::OK),
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
    let writer = app_router(load_config(dir.path())).await;
    let reader = app_router(load_config(dir.path())).await;
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
async fn a_stored_rule_change_with_unknown_fields_does_not_fail_closed() {
    let dir = TempDir::new().unwrap();
    let storage_dir = dir.path().join("storage/.rule-overrides/v0");
    std::fs::create_dir_all(&storage_dir).unwrap();
    let change_with_unknown = json!({
        "access": ["$authenticated"],
        "futureFieldFromNewerReplica": true,
        "packages": {
            "@secret/*": {
                "access": ["root"],
                "unknownPackageField": 123
            }
        }
    });
    std::fs::write(
        storage_dir.join("local.json"),
        serde_json::to_vec(&change_with_unknown).unwrap(),
    )
    .unwrap();

    let app = app_router(load_config(dir.path())).await;
    let root = add_user(&app, "root").await;
    let (status, rules) = send(&app, "GET", RULES, Some(&root), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rules["access"], json!(["$authenticated"]));
    assert_eq!(rules["packages"]["@secret/*"]["access"], json!(["root"]));
}

#[tokio::test]
async fn moving_registry_into_ecosystem_group_preserves_rules() {
    let dir = TempDir::new().unwrap();
    let app1 = app_router(load_config(dir.path())).await;
    let root = add_user(&app1, "root").await;
    let change = json!({ "access": ["$authenticated"] });
    assert_eq!(send(&app1, "PUT", RULES, Some(&root), Some(change)).await.0, StatusCode::OK);

    let grouped_config_yaml = "\
auth:
  admins: [root]
  htpasswd:
    max_users: 100
registries:
  npm:
    local:
      type: hosted
      org: \"\"
      rulesManagedBy: api
      teams:
        readers: [carol]
      packages:
        '@secret/*':
          access: [$authenticated]
        '**': {}
  fixed:
    type: hosted
    org: fixed
    packages:
      '@fixed/*': {}
  main:
    type: router
    sources: [fixed, 'npm/local']
defaultRegistry: main
";
    let storage = dir.path().join("storage");
    let path = dir.path().join("config_grouped.yaml");
    std::fs::write(&path, format!("storage: {}\n{grouped_config_yaml}", storage.display()))
        .unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let config2 =
        Config::from_yaml(&path, listen, Some("http://example.test".to_string())).unwrap();
    let app2 = app_router(config2).await;
    let root2 = add_user(&app2, "root").await;

    let (status, rules) =
        send(&app2, "GET", "/-/pnpr/v0/admin/rules/npm/local", Some(&root2), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rules["access"], json!(["$authenticated"]));
}

#[tokio::test]
async fn cargo_and_pypi_hosted_registries_reject_unpublish_rules() {
    let dir = TempDir::new().unwrap();
    let config_yaml = "\
auth:
  admins: [root]
  htpasswd:
    max_users: 100
registries:
  cargo:
    crates:
      type: hosted
      rulesManagedBy: api
      packages:
        'foo': {}
  pypi:
    wheels:
      type: hosted
      rulesManagedBy: api
      packages:
        'bar': {}
  main:
    type: router
    sources: ['cargo/crates']
defaultRegistry: main
";
    let storage = dir.path().join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    let path = dir.path().join("config.yaml");
    std::fs::write(&path, format!("storage: {}\n{config_yaml}", storage.display())).unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let config = Config::from_yaml(&path, listen, Some("http://example.test".to_string())).unwrap();
    let app = app_router(config).await;
    let root = add_user(&app, "root").await;

    let unpublish_body = json!({
        "packages": {
            "foo": {
                "unpublish": ["root"],
            },
        },
    });
    let (status, bytes) = send_raw(
        &app,
        "PUT",
        "/-/pnpr/v0/admin/rules/cargo/crates",
        Some(&root),
        Some(unpublish_body),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let msg = String::from_utf8_lossy(&bytes);
    assert!(
        msg.contains("serves cargo, which does not support unpublishing"),
        "expected unpublish error, got: {msg}",
    );

    let pypi_body = json!({
        "packages": {
            "bar": {
                "unpublish": ["root"],
            },
        },
    });
    let (status, bytes) =
        send_raw(&app, "PUT", "/-/pnpr/v0/admin/rules/pypi/wheels", Some(&root), Some(pypi_body))
            .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let msg = String::from_utf8_lossy(&bytes);
    assert!(
        msg.contains("serves pypi, which does not support unpublishing"),
        "expected unpublish error, got: {msg}",
    );
}

#[tokio::test]
async fn pre_upgrade_stored_unpublish_override_on_cargo_does_not_lock_out_registry() {
    let dir = TempDir::new().unwrap();
    let config_yaml = "\
auth:
  admins: [root]
  htpasswd:
    max_users: 100
registries:
  cargo:
    crates:
      type: hosted
      rulesManagedBy: api
      packages:
        'foo': {}
  main:
    type: router
    sources: ['cargo/crates']
defaultRegistry: main
";
    let storage = dir.path().join("storage");
    let storage_dir = storage.join(".rule-overrides/v0");
    std::fs::create_dir_all(&storage_dir).unwrap();
    let change_with_unpublish = json!({
        "packages": {
            "foo": {
                "access": ["$authenticated"],
                "unpublish": ["root"],
            },
        },
    });
    let target_dir = storage_dir.join("cargo");
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::write(
        target_dir.join("crates.json"),
        serde_json::to_vec(&change_with_unpublish).unwrap(),
    )
    .unwrap();

    let path = dir.path().join("config.yaml");
    std::fs::write(&path, format!("storage: {}\n{config_yaml}", storage.display())).unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let config = Config::from_yaml(&path, listen, Some("http://example.test".to_string())).unwrap();
    let app = app_router(config).await;
    let root = add_user(&app, "root").await;

    let (status, rules) =
        send(&app, "GET", "/-/pnpr/v0/admin/rules/cargo/crates", Some(&root), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rules["packages"]["foo"]["access"], json!(["$authenticated"]));
    assert_eq!(rules["packages"]["foo"]["unpublish"], serde_json::Value::Null);
}
