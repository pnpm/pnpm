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
use pnpr::{
    AuthState, Config, PasswordChange, TokenBackend, TokenStore, UpsertOutcome, UserBackend,
    UserCreation, UserRemoval, UserStore, cli_commands::Command, router, router_with_auth,
};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::Path,
    sync::Arc,
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

/// A config with a file-backed htpasswd store, which `pnpr add-user` needs.
fn htpasswd_config(dir: &Path) -> Config {
    let storage = dir.join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    std::fs::write(dir.join("htpasswd"), "").unwrap();
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
    let path = dir.join("config.yaml");
    std::fs::write(&path, yaml).unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    Config::from_yaml(&path, listen, Some("http://example.test".to_string())).unwrap()
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
    let config = htpasswd_config(dir.path());

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

#[tokio::test]
async fn cli_add_user_rejects_empty_password() {
    let dir = TempDir::new().unwrap();
    let storage = dir.path().join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    let htpasswd = dir.path().join("htpasswd");
    std::fs::write(&htpasswd, "").unwrap();
    let yaml = format!(
        "\
storage: {}
auth:
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

    let cmd =
        Command::AddUser { username: "admin_alice".to_string(), password: Some(String::new()) };
    let err = pnpr::cli_commands::run_command(&config, cmd).await.unwrap_err();
    assert!(err.to_string().contains("password cannot be empty"), "unexpected error: {err}");
}

/// A user store whose login finds the account removed after the login looked
/// it up, as if an admin removed it in between. With `rewritten_to`, the admin
/// then sets that password on the account the login created.
struct RemovedDuringLogin {
    users: UserStore,
    rewritten_to: Option<&'static str>,
}

#[async_trait::async_trait]
impl UserBackend for RemovedDuringLogin {
    async fn add_or_login(
        &self,
        username: &str,
        password: &str,
    ) -> pnpr::Result<(UpsertOutcome, String)> {
        self.users.delete_user(username).await?;
        let created = self.users.add_or_login(username, password).await?;
        if let Some(rewritten_to) = self.rewritten_to {
            self.users.set_password(username, rewritten_to).await?;
        }
        Ok(created)
    }

    async fn list_users(&self) -> pnpr::Result<Vec<String>> {
        self.users.list_users().await
    }

    async fn password_hash(&self, username: &str) -> pnpr::Result<Option<String>> {
        self.users.password_hash(username).await
    }

    async fn create_user(&self, username: &str, password: &str) -> pnpr::Result<UserCreation> {
        self.users.create_user(username, password).await
    }

    async fn set_password(&self, username: &str, password: &str) -> pnpr::Result<PasswordChange> {
        self.users.set_password(username, password).await
    }

    async fn delete_user(&self, username: &str) -> pnpr::Result<UserRemoval> {
        self.users.delete_user(username).await
    }
}

#[tokio::test]
async fn a_login_racing_the_removal_of_a_reserved_account_keeps_no_account() {
    let dir = TempDir::new().unwrap();
    let users = Arc::new(RemovedDuringLogin { users: UserStore::in_memory(), rewritten_to: None });
    users.users.create_user("admin_alice", "correct_alice_password").await.unwrap();
    let tokens = Arc::new(TokenStore::in_memory());
    let auth = AuthState { users: Arc::clone(&users) as _, tokens: Arc::clone(&tokens) as _ };
    let app = router_with_auth(test_config(dir.path()), auth);

    let (status, text, _) = put_login(&app, "admin_alice", "malicious_password").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(text, r#"Username "admin_alice" is reserved"#);
    assert!(
        users.users
            .password_hash("admin_alice")
            .await
            .unwrap()
            .is_none(),
    );
    assert!(
        tokens
            .list_for_user("admin_alice")
            .await
            .unwrap()
            .is_empty(),
    );
}

#[tokio::test]
async fn cli_add_user_revokes_tokens_left_under_the_name() {
    let dir = TempDir::new().unwrap();
    let config = htpasswd_config(dir.path());
    let left_behind = AuthState::load(&config.identity.auth, &config.identity.backend)
        .await
        .unwrap()
        .tokens
        .issue("admin_alice")
        .await
        .unwrap();

    let cmd = Command::AddUser {
        username: "admin_alice".to_string(),
        password: Some("bootstrap_pass".to_string()),
    };
    pnpr::cli_commands::run_command(&config, cmd).await.unwrap();

    let auth = AuthState::load(&config.identity.auth, &config.identity.backend)
        .await
        .unwrap();
    assert_eq!(auth.tokens.lookup(&left_behind).await.unwrap(), None);
}

#[tokio::test]
async fn a_login_racing_the_removal_of_a_reserved_account_keeps_an_admin_rewrite() {
    let dir = TempDir::new().unwrap();
    let users = Arc::new(RemovedDuringLogin {
        users: UserStore::in_memory(),
        rewritten_to: Some("admin_rewritten_password"),
    });
    users.users.create_user("admin_alice", "correct_alice_password").await.unwrap();
    let auth =
        AuthState { users: Arc::clone(&users) as _, tokens: Arc::new(TokenStore::in_memory()) };
    let app = router_with_auth(test_config(dir.path()), auth);

    let (status, _, _) = put_login(&app, "admin_alice", "malicious_password").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (outcome, _) =
        users.users.add_or_login("admin_alice", "admin_rewritten_password").await.unwrap();
    assert_eq!(outcome, UpsertOutcome::LoggedIn);
}
