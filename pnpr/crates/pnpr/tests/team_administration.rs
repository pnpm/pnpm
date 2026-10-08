//! Team administration through the npm team API on a registry whose teams
//! the API manages (`teamsManagedBy: api`): who may edit the roster, how an
//! edit reaches the `team:` rules, and that the roster lives in the hosted
//! store rather than in the process.

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
    teamsManagedBy: api
    teams:
      readers: []
    packages:
      '@secret/*':
        access: [team:readers]
      '**': {}
  main:
    type: router
    sources: [local]
defaultRegistry: main
";

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

async fn team_names(app: &axum::Router, token: &str) -> Value {
    let (status, teams) = send(app, "GET", "/-/org/acme/team", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    teams
}

#[tokio::test]
async fn admin_edits_reach_team_rules_at_once() {
    let dir = TempDir::new().unwrap();
    let app = router(load_config(dir.path()));
    let root = add_user(&app, "root").await;
    let bob = add_user(&app, "bob").await;

    // The package does not exist, so 404 proves the read was authorized and
    // 403 proves it was not.
    let read = || send(&app, "GET", "/@secret/thing", Some(&bob), None);
    assert_eq!(read().await.0, StatusCode::FORBIDDEN);

    let add = json!({ "user": "bob" });
    let path = "/-/team/acme/readers/user";
    assert_eq!(send(&app, "PUT", path, Some(&root), Some(add)).await.0, StatusCode::CREATED);
    assert_eq!(read().await.0, StatusCode::NOT_FOUND);
    let (_, members) = send(&app, "GET", path, Some(&root), None).await;
    assert_eq!(members, json!([{ "name": "bob" }]));

    let remove = json!({ "user": "bob" });
    assert_eq!(
        send(&app, "DELETE", path, Some(&root), Some(remove)).await.0,
        StatusCode::NO_CONTENT,
    );
    assert_eq!(read().await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn only_admins_edit_the_roster() {
    let dir = TempDir::new().unwrap();
    let app = router(load_config(dir.path()));
    let bob = add_user(&app, "bob").await;
    let create = json!({ "name": "ops" });

    let (status, _) = send(&app, "PUT", "/-/org/acme/team", Some(&bob), Some(create.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(&app, "PUT", "/-/org/acme/team", None, Some(create)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(team_names(&app, &bob).await, json!([{ "name": "readers" }]));
}

#[tokio::test]
async fn roster_edits_refuse_conflicting_state() {
    let dir = TempDir::new().unwrap();
    let app = router(load_config(dir.path()));
    let root = add_user(&app, "root").await;
    let create = json!({ "name": "ops" });

    let (status, team) =
        send(&app, "PUT", "/-/org/acme/team", Some(&root), Some(create.clone())).await;
    assert_eq!((status, team), (StatusCode::CREATED, json!({ "name": "ops" })));
    let (status, _) = send(&app, "PUT", "/-/org/acme/team", Some(&root), Some(create)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    // A `packages:` rule names `readers`, so it cannot be destroyed.
    let (status, _) = send(&app, "DELETE", "/-/team/acme/readers", Some(&root), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = send(&app, "DELETE", "/-/team/acme/nope", Some(&root), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let add = json!({ "user": "bob" });
    let (status, _) = send(&app, "PUT", "/-/team/acme/nope/user", Some(&root), Some(add)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(&app, "DELETE", "/-/team/acme/ops", Some(&root), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(team_names(&app, &root).await, json!([{ "name": "readers" }]));
}

#[tokio::test]
async fn a_new_process_serves_the_stored_roster() {
    let dir = TempDir::new().unwrap();
    let app = router(load_config(dir.path()));
    let root = add_user(&app, "root").await;
    let create = json!({ "name": "ops" });
    assert_eq!(
        send(&app, "PUT", "/-/org/acme/team", Some(&root), Some(create)).await.0,
        StatusCode::CREATED,
    );

    let restarted = router(load_config(dir.path()));
    let bob = add_user(&restarted, "bob").await;
    assert_eq!(
        team_names(&restarted, &bob).await,
        json!([{ "name": "readers" }, { "name": "ops" }]),
    );
}

#[tokio::test]
async fn directory_reports_admin_and_team_management() {
    let dir = TempDir::new().unwrap();
    let app = router(load_config(dir.path()));
    let root = add_user(&app, "root").await;
    let bob = add_user(&app, "bob").await;

    let (_, directory) = send(&app, "GET", "/-/pnpr/v0/registries", Some(&root), None).await;
    assert_eq!(directory["admin"], json!(true));
    let local = directory["registries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "local")
        .expect("local is listed");
    assert_eq!(local["teamsManagedBy"], json!("api"));
    let (_, directory) = send(&app, "GET", "/-/pnpr/v0/registries", Some(&bob), None).await;
    assert_eq!(directory["admin"], json!(false));
}

#[tokio::test]
async fn team_edits_refuse_oversized_bodies() {
    let dir = TempDir::new().unwrap();
    let app = router(load_config(dir.path()));
    let body = json!({ "name": "x".repeat(100 * 1024) });

    let (status, _) = send(&app, "PUT", "/-/org/acme/team", None, Some(body)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}
