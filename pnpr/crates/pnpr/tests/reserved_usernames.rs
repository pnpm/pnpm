//! Tests ensuring reserved usernames cannot be squatted via self-registration
//! (`PUT /-/user/org.couchdb.user:{name}`), including:
//! - configured admins (`auth.admins`)
//! - OIDC login bindings (`auth.oidc[].login.users[].username`)
//! - OIDC workload bindings (`auth.oidc[].workloads[].identity.username`)
//!
//! Existing accounts logging in with their correct password continue to work.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::{AuthState, Config, cli_commands::Command, router, router_with_auth};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::Path,
};
use tempfile::TempDir;
use tower::ServiceExt;

fn test_config(dir: &Path) -> Config {
    let storage = dir.join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    let yaml = format!(
        "\
storage: {}
auth:
  admins: [admin_alice]
  htpasswd:
    max_users: 100
  oidc:
    - name: idp
      issuer: https://issuer.example.com
      audience: pnpr
      login:
        users:
          - subject: sub-100
            username: oidc_bob
      workloads:
        - identity:
            subject: repo:pnpm/pnpm
            username: release-ci
          registry: local
          packages: ['@pnpm/pkg']
registries:
  local:
    type: hosted
",
        storage.display(),
    );
    let path = dir.join("config.yaml");
    std::fs::write(&path, yaml).unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    Config::from_yaml(&path, listen, Some("https://example.test".to_string())).unwrap()
}

async fn send_request(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    content_type: &str,
    body: Option<Value>,
) -> (StatusCode, String, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", content_type);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let body_bytes =
        body.map(|payload| Body::from(payload.to_string())).unwrap_or_else(Body::empty);
    let response = app
        .clone()
        .oneshot(builder.body(body_bytes).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, text, json)
}

async fn put_login(
    app: &axum::Router,
    username: &str,
    password: &str,
) -> (StatusCode, String, Value) {
    let body = json!({
        "name": username,
        "password": password,
        "type": "user",
        "roles": [],
    });
    let path = format!("/-/user/org.couchdb.user:{username}");
    send_request(app, "PUT", &path, None, "application/json", Some(body)).await
}

#[tokio::test]
async fn cannot_self_register_configured_admin() {
    let dir = TempDir::new().unwrap();
    let app = router(test_config(dir.path()));
    let (status, text, _) = put_login(&app, "admin_alice", "malicious_password").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(text, r#"Username "admin_alice" is reserved"#);
}

#[tokio::test]
async fn cannot_self_register_oidc_login_bound_user() {
    let dir = TempDir::new().unwrap();
    let app = router(test_config(dir.path()));
    let (status, text, _) = put_login(&app, "oidc_bob", "malicious_password").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(text, r#"Username "oidc_bob" is reserved"#);
}

#[tokio::test]
async fn cannot_self_register_oidc_workload_bound_user() {
    let dir = TempDir::new().unwrap();
    let app = router(test_config(dir.path()));
    let (status, text, _) = put_login(&app, "release-ci", "malicious_password").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(text, r#"Username "release-ci" is reserved"#);
}

#[tokio::test]
async fn existing_admin_and_normal_accounts_can_log_in() {
    let dir = TempDir::new().unwrap();
    let config = test_config(dir.path());
    let auth = AuthState::in_memory_with_max_users(config.identity.auth.htpasswd.max_users);
    auth.users.create_user("admin_alice", "correct_alice_password").await.unwrap();
    let app = router_with_auth(config, auth);

    let (status, _, payload) = put_login(&app, "admin_alice", "correct_alice_password").await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(payload["token"].is_string());

    let (status, _, _) = put_login(&app, "admin_alice", "wrong_alice_password").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _, payload) = put_login(&app, "regular_dan", "dan_password").await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(payload["token"].is_string());
}

#[tokio::test]
async fn bootstrap_cli_add_user_creates_and_updates_account() {
    let dir = TempDir::new().unwrap();
    let storage = dir.path().join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    let htpasswd = dir.path().join("htpasswd");
    std::fs::write(&htpasswd, "").unwrap();
    let yaml = format!(
        "\
storage: {}
auth:
  admins: [admin_alice]
  htpasswd:
    file: ./htpasswd
    max_users: 100
registries:
  local:
    type: hosted
",
        storage.display(),
    );
    let path = dir.path().join("config.yaml");
    std::fs::write(&path, yaml).unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let config = Config::from_yaml(&path, listen, Some("http://example.test".to_string())).unwrap();

    let create_cmd = Command::AddUser {
        username: "admin_alice".to_string(),
        password: Some("bootstrap_pass".to_string()),
    };
    pnpr::cli_commands::run_command(&config, create_cmd).await.unwrap();

    let auth = AuthState::load(&config.identity.auth, &config.identity.backend)
        .await
        .unwrap();
    let app = router_with_auth(config.clone(), auth);

    let (status, _, payload) = put_login(&app, "admin_alice", "bootstrap_pass").await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(payload["token"].is_string());

    let update_cmd = Command::AddUser {
        username: "admin_alice".to_string(),
        password: Some("new_bootstrap_pass".to_string()),
    };
    pnpr::cli_commands::run_command(&config, update_cmd).await.unwrap();

    let reloaded_auth = AuthState::load(&config.identity.auth, &config.identity.backend)
        .await
        .unwrap();
    let reloaded_app = router_with_auth(config, reloaded_auth);

    let (status, _, _) = put_login(&reloaded_app, "admin_alice", "bootstrap_pass").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _, payload) = put_login(&reloaded_app, "admin_alice", "new_bootstrap_pass").await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(payload["token"].is_string());
}

#[tokio::test]
async fn cli_add_user_rejects_in_memory_auth_backend() {
    let dir = TempDir::new().unwrap();
    let config = test_config(dir.path());
    let cmd = Command::AddUser {
        username: "admin_alice".to_string(),
        password: Some("secret".to_string()),
    };
    let err = pnpr::cli_commands::run_command(&config, cmd).await.unwrap_err();
    assert!(
        err.to_string().contains("standalone add-user requires a persistent auth backend"),
        "unexpected error: {err}",
    );
}
