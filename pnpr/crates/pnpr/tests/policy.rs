//! Integration tests for the YAML-driven access policy: the
//! `packages:` `access` / `publish` tokens compile into the runtime
//! policy and gate requests, including the `$anonymous` rule.
//! Static-mode (no upstream) to keep the tests hermetic.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::{Config, router};
use serde_json::{Value, json};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use tempfile::TempDir;
use tower::ServiceExt;

fn listen() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873))
}

/// Write a `config.yaml` (with `packages_block` spliced in) into a
/// fresh tempdir and load it through the real `from_yaml` path, so
/// the test exercises YAML parsing → policy compilation end to end.
fn config_from_yaml(packages_block: &str) -> (TempDir, Config) {
    let dir = TempDir::new().unwrap();
    let storage = dir.path().join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    let yaml = format!(
        "storage: {}\nuplinks: {{}}\nauth:\n  htpasswd:\n    max_users: 100\n{packages_block}\n",
        storage.display(),
    );
    let path = dir.path().join("config.yaml");
    std::fs::write(&path, yaml).unwrap();
    let config =
        Config::from_yaml(&path, listen(), Some("http://example.test".to_string())).unwrap();
    (dir, config)
}

fn get(path: &str) -> Request<Body> {
    Request::get(path).body(Body::empty()).unwrap()
}

fn get_auth(path: &str, token: &str) -> Request<Body> {
    Request::get(path)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

async fn status_of(app: axum::Router, req: Request<Body>) -> StatusCode {
    app.oneshot(req).await.unwrap().status()
}

async fn add_user_and_get_token(app: &axum::Router, username: &str, password: &str) -> String {
    let path = format!("/-/user/org.couchdb.user:{username}");
    let body = json!({
        "name": username,
        "password": password,
        "email": "u@e.test",
        "type": "user",
        "roles": [],
    });
    let req = Request::put(&path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();
    payload["token"].as_str().expect("token in response").to_string()
}

#[tokio::test]
async fn authenticated_access_token_from_yaml_gates_anonymous_reads() {
    let (_dir, config) = config_from_yaml(
        "packages:\n  '@secret/*':\n    access: $authenticated\n  '**':\n    access: $all\n",
    );
    let app = router(config);

    // `@secret/*` requires auth: an anonymous read is 401.
    assert_eq!(status_of(app.clone(), get("/@secret/thing")).await, StatusCode::UNAUTHORIZED);
    // A `**` ($all) package is readable anonymously — it's just absent
    // on disk, so 404, which proves the access check passed.
    assert_eq!(status_of(app, get("/lodash")).await, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn anonymous_rule_admits_anonymous_and_forbids_authenticated() {
    let (_dir, config) = config_from_yaml(
        "packages:\n  '@anon/*':\n    access: $anonymous\n  '**':\n    access: $all\n",
    );
    let app = router(config);

    // Anonymous read passes the access check (404 = allowed but absent).
    assert_eq!(status_of(app.clone(), get("/@anon/x")).await, StatusCode::NOT_FOUND);

    // An authenticated caller is outside the `$anonymous` group → 403.
    let token = add_user_and_get_token(&app, "alice", "secret").await;
    assert_eq!(status_of(app, get_auth("/@anon/x", &token)).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn username_in_access_list_grants_only_that_user() {
    // verdaccio per-user access: `@team/*` is restricted to `alice`.
    let (_dir, config) =
        config_from_yaml("packages:\n  '@team/*':\n    access: alice\n  '**':\n    access: $all\n");
    let app = router(config);

    // Anonymous: no creds for a name-gated package → 401.
    assert_eq!(status_of(app.clone(), get("/@team/x")).await, StatusCode::UNAUTHORIZED);

    // A different authenticated user is not `alice` → 403.
    let bob = add_user_and_get_token(&app, "bob", "secret").await;
    assert_eq!(status_of(app.clone(), get_auth("/@team/x", &bob)).await, StatusCode::FORBIDDEN);

    // `alice` is on the list → access granted (404 = allowed but absent).
    let alice = add_user_and_get_token(&app, "alice", "secret").await;
    assert_eq!(status_of(app, get_auth("/@team/x", &alice)).await, StatusCode::NOT_FOUND);
}
