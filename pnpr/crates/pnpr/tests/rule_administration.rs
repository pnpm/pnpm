//! The admin API for package rules (`/-/pnpr/v0/admin/rules/{registry}`) on a
//! registry with `rulesManagedBy: api`: what a change does to requests, who
//! may make one, and that changes live in the hosted store.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::{Config, router};
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

const RULES: &str = "/-/pnpr/v0/admin/rules/local";

fn load_config(dir: &Path) -> Config {
    let storage = dir.join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    let path = dir.join("config.yaml");
    std::fs::write(&path, format!("storage: {}\n{CONFIG}", storage.display())).unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    Config::from_yaml(&path, listen, Some("http://example.test".to_string())).unwrap()
}

async fn send(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let body = body.map_or_else(Body::empty, |body| Body::from(body.to_string()));
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
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
    send(app, "GET", "/@secret/thing", Some(token), None).await.0
}

#[tokio::test]
async fn a_change_applies_at_once_and_a_reset_undoes_it() {
    let dir = TempDir::new().unwrap();
    let app = router(load_config(dir.path()));
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
    let app = router(load_config(dir.path()));
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
    let app = router(load_config(dir.path()));
    let root = add_user(&app, "root").await;
    let bob = add_user(&app, "bob").await;
    let change = json!({ "access": ["$authenticated"] });

    assert_eq!(
        send(&app, "PUT", RULES, Some(&bob), Some(change.clone())).await.0,
        StatusCode::FORBIDDEN,
    );
    assert_eq!(send(&app, "GET", RULES, None, None).await.0, StatusCode::UNAUTHORIZED);
    let fixed = "/-/pnpr/v0/admin/rules/fixed";
    assert_eq!(
        send(&app, "PUT", fixed, Some(&root), Some(change.clone())).await.0,
        StatusCode::FORBIDDEN,
    );
    let (status, rules) = send(&app, "GET", fixed, Some(&root), None).await;
    assert_eq!((status, &rules["rulesManagedBy"]), (StatusCode::OK, &json!("config")));
    let missing = "/-/pnpr/v0/admin/rules/missing";
    assert_eq!(
        send(&app, "PUT", missing, Some(&root), Some(change)).await.0,
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn a_new_process_applies_the_stored_changes() {
    let dir = TempDir::new().unwrap();
    let app = router(load_config(dir.path()));
    let root = add_user(&app, "root").await;
    let change = json!({ "packages": { "@secret/*": { "access": ["root"] } } });
    assert_eq!(send(&app, "PUT", RULES, Some(&root), Some(change)).await.0, StatusCode::OK);

    let restarted = router(load_config(dir.path()));
    let bob = add_user(&restarted, "bob").await;
    assert_eq!(read_secret(&restarted, &bob).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn unreadable_stored_changes_close_the_registry() {
    let dir = TempDir::new().unwrap();
    let record = dir.path().join("storage/.rule-overrides/v0/local.json");
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    std::fs::write(&record, "not rules").unwrap();
    let app = router(load_config(dir.path()));
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
