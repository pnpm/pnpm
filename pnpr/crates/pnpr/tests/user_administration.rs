//! The admin API for accounts (`/-/pnpr/v0/admin/users`): who may use it, and
//! what creating, re-keying, and removing an account does to logins and
//! tokens.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::{
    AuthState, Config, RegistryError, TokenBackend, TokenRecord, TokenStore, UpsertOutcome,
    UserBackend, UserStore, router, router_with_auth,
};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::Arc,
};
use tempfile::TempDir;
use tower::ServiceExt;

/// One self-registration slot, which `root` takes, so every other account
/// has to come from the admin API.
fn load_config(dir: &TempDir) -> Config {
    let storage = dir.path().join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    let yaml = format!(
        "storage: {}\nauth:\n  admins: [root]\n  htpasswd:\n    file: ./htpasswd\n    max_users: 1\n",
        storage.display(),
    );
    let path = dir.path().join("config.yaml");
    std::fs::write(&path, yaml).unwrap();
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

/// `npm login`: a token for an existing account, or the error status.
async fn login(app: &axum::Router, username: &str, password: &str) -> Result<String, StatusCode> {
    let body = json!({ "name": username, "password": password, "type": "user", "roles": [] });
    let path = format!("/-/user/org.couchdb.user:{username}");
    let (status, payload) = send(app, "PUT", &path, None, Some(body)).await;
    if status != StatusCode::CREATED {
        return Err(status);
    }
    Ok(payload["token"]
        .as_str()
        .expect("token in response")
        .to_string())
}

async fn whoami(app: &axum::Router, token: &str) -> StatusCode {
    send(app, "GET", "/-/whoami", Some(token), None).await.0
}

#[tokio::test]
async fn admins_create_accounts_past_the_registration_cap() {
    let dir = TempDir::new().unwrap();
    let app = router(load_config(&dir));
    let root = login(&app, "root", "secret").await.unwrap();
    assert_eq!(login(&app, "bob", "first").await, Err(StatusCode::FORBIDDEN));

    let body = json!({ "password": "first" });
    let (status, _) =
        send(&app, "PUT", "/-/pnpr/v0/admin/users/bob", Some(&root), Some(body)).await;
    assert_eq!(status, StatusCode::CREATED);
    let bob = login(&app, "bob", "first").await.unwrap();
    assert_eq!(whoami(&app, &bob).await, StatusCode::OK);

    let (_, users) = send(&app, "GET", "/-/pnpr/v0/admin/users", Some(&root), None).await;
    assert_eq!(users, json!({ "users": [{ "name": "bob" }, { "name": "root" }] }));
}

#[tokio::test]
async fn a_new_password_replaces_the_old_one() {
    let dir = TempDir::new().unwrap();
    let app = router(load_config(&dir));
    let root = login(&app, "root", "secret").await.unwrap();
    let path = "/-/pnpr/v0/admin/users/bob";
    send(&app, "PUT", path, Some(&root), Some(json!({ "password": "first" }))).await;

    let (status, _) =
        send(&app, "PUT", path, Some(&root), Some(json!({ "password": "second" }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(login(&app, "bob", "first").await, Err(StatusCode::UNAUTHORIZED));
    assert!(login(&app, "bob", "second").await.is_ok());
}

#[tokio::test]
async fn removing_an_account_revokes_its_tokens() {
    let dir = TempDir::new().unwrap();
    let app = router(load_config(&dir));
    let root = login(&app, "root", "secret").await.unwrap();
    let path = "/-/pnpr/v0/admin/users/bob";
    send(&app, "PUT", path, Some(&root), Some(json!({ "password": "first" }))).await;
    let bob = login(&app, "bob", "first").await.unwrap();

    assert_eq!(send(&app, "DELETE", path, Some(&root), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(whoami(&app, &bob).await, StatusCode::UNAUTHORIZED);
    assert_eq!(login(&app, "bob", "first").await, Err(StatusCode::FORBIDDEN));
    assert_eq!(send(&app, "DELETE", path, Some(&root), None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admins_revoke_one_token_of_one_user() {
    let dir = TempDir::new().unwrap();
    let app = router(load_config(&dir));
    let root = login(&app, "root", "secret").await.unwrap();
    send(&app, "PUT", "/-/pnpr/v0/admin/users/bob", Some(&root), Some(json!({ "password": "x" })))
        .await;
    let bob = login(&app, "bob", "x").await.unwrap();

    let (status, tokens) =
        send(&app, "GET", "/-/pnpr/v0/admin/users/bob/tokens", Some(&root), None).await;
    assert_eq!(status, StatusCode::OK);
    let key = tokens["objects"][0]["key"]
        .as_str()
        .unwrap()
        .to_string();
    let wrong_owner = format!("/-/pnpr/v0/admin/users/root/tokens/{key}");
    assert_eq!(
        send(&app, "DELETE", &wrong_owner, Some(&root), None).await.0,
        StatusCode::NOT_FOUND,
    );
    assert_eq!(whoami(&app, &bob).await, StatusCode::OK);

    let path = format!("/-/pnpr/v0/admin/users/bob/tokens/{key}");
    assert_eq!(send(&app, "DELETE", &path, Some(&root), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(whoami(&app, &bob).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn only_admins_use_the_user_api() {
    let dir = TempDir::new().unwrap();
    let app = router(load_config(&dir));
    let root = login(&app, "root", "secret").await.unwrap();
    send(&app, "PUT", "/-/pnpr/v0/admin/users/bob", Some(&root), Some(json!({ "password": "x" })))
        .await;
    let bob = login(&app, "bob", "x").await.unwrap();

    let requests = [
        ("GET", "/-/pnpr/v0/admin/users", None),
        ("PUT", "/-/pnpr/v0/admin/users/carol", Some(json!({ "password": "x" }))),
        ("DELETE", "/-/pnpr/v0/admin/users/root", None),
        ("GET", "/-/pnpr/v0/admin/users/root/tokens", None),
    ];
    for (method, path, body) in requests {
        assert_eq!(
            send(&app, method, path, Some(&bob), body.clone()).await.0,
            StatusCode::FORBIDDEN,
        );
        assert_eq!(send(&app, method, path, None, body).await.0, StatusCode::UNAUTHORIZED);
    }
}

/// A user store whose one account is removed between a login's password
/// check and its token.
struct RemovedDuringLogin;

#[async_trait::async_trait]
impl UserBackend for RemovedDuringLogin {
    async fn add_or_login(
        &self,
        username: &str,
        _password: &str,
    ) -> pnpr::Result<(UpsertOutcome, String)> {
        Ok((UpsertOutcome::LoggedIn, username.to_string()))
    }

    async fn list_users(&self) -> pnpr::Result<Vec<String>> {
        Ok(Vec::new())
    }

    async fn password_hash(&self, _username: &str) -> pnpr::Result<Option<String>> {
        Ok(None)
    }

    async fn create_user(&self, _username: &str, _password: &str) -> pnpr::Result<bool> {
        Ok(false)
    }

    async fn set_password(&self, _username: &str, _password: &str) -> pnpr::Result<bool> {
        Ok(false)
    }

    async fn delete_user(&self, _username: &str) -> pnpr::Result<bool> {
        Ok(false)
    }
}

#[tokio::test]
async fn a_login_racing_a_removal_keeps_no_token() {
    let dir = TempDir::new().unwrap();
    let tokens = Arc::new(TokenStore::in_memory());
    let auth = AuthState { users: Arc::new(RemovedDuringLogin), tokens: Arc::clone(&tokens) as _ };
    let app = router_with_auth(load_config(&dir), auth);

    assert_eq!(login(&app, "bob", "x").await, Err(StatusCode::UNAUTHORIZED));
    assert!(
        tokens
            .list_for_user("bob")
            .await
            .unwrap()
            .is_empty(),
    );
}

/// A token store that cannot revoke.
struct FailingRevocation(TokenStore);

#[async_trait::async_trait]
impl TokenBackend for FailingRevocation {
    async fn issue(&self, username: &str) -> pnpr::Result<String> {
        self.0.issue(username).await
    }

    async fn lookup(&self, raw: &str) -> pnpr::Result<Option<String>> {
        self.0.lookup(raw).await
    }

    async fn find_by_key(&self, key: &str) -> pnpr::Result<Option<TokenRecord>> {
        self.0.find_by_key(key).await
    }

    async fn list_for_user(&self, username: &str) -> pnpr::Result<Vec<(String, TokenRecord)>> {
        self.0.list_for_user(username).await
    }

    async fn revoke_by_key(&self, _key: &str) -> pnpr::Result<Option<TokenRecord>> {
        Err(RegistryError::Internal { reason: "revocation is down".to_string() })
    }
}

#[tokio::test]
async fn a_failed_revocation_keeps_the_account() {
    let dir = TempDir::new().unwrap();
    let users = Arc::new(UserStore::in_memory());
    let auth = AuthState {
        users: Arc::clone(&users) as _,
        tokens: Arc::new(FailingRevocation(TokenStore::in_memory())),
    };
    let app = router_with_auth(load_config(&dir), auth);
    let root = login(&app, "root", "secret").await.unwrap();
    users.create_user("bob", "x").await.unwrap();
    login(&app, "bob", "x").await.unwrap();

    let (status, _) = send(&app, "DELETE", "/-/pnpr/v0/admin/users/bob", Some(&root), None).await;
    assert!(status.is_server_error(), "{status}");
    assert!(
        users
            .password_hash("bob")
            .await
            .unwrap()
            .is_some(),
    );
}

/// A user store whose one account is removed and created again between a
/// login's password check and its token: the hash the login verified is
/// followed by another.
struct ReplacedDuringLogin(std::sync::Mutex<Vec<&'static str>>);

#[async_trait::async_trait]
impl UserBackend for ReplacedDuringLogin {
    async fn add_or_login(
        &self,
        username: &str,
        _password: &str,
    ) -> pnpr::Result<(UpsertOutcome, String)> {
        Ok((UpsertOutcome::LoggedIn, username.to_string()))
    }

    async fn list_users(&self) -> pnpr::Result<Vec<String>> {
        Ok(Vec::new())
    }

    async fn password_hash(&self, _username: &str) -> pnpr::Result<Option<String>> {
        Ok(self.0
            .lock()
            .unwrap()
            .pop()
            .map(str::to_string))
    }

    async fn create_user(&self, _username: &str, _password: &str) -> pnpr::Result<bool> {
        Ok(false)
    }

    async fn set_password(&self, _username: &str, _password: &str) -> pnpr::Result<bool> {
        Ok(false)
    }

    async fn delete_user(&self, _username: &str) -> pnpr::Result<bool> {
        Ok(false)
    }
}

#[tokio::test]
async fn an_old_password_gets_no_token_for_a_recreated_account() {
    let dir = TempDir::new().unwrap();
    let tokens = Arc::new(TokenStore::in_memory());
    let users = ReplacedDuringLogin(std::sync::Mutex::new(vec!["new hash", "old hash"]));
    let auth = AuthState { users: Arc::new(users), tokens: Arc::clone(&tokens) as _ };
    let app = router_with_auth(load_config(&dir), auth);

    assert_eq!(login(&app, "bob", "old").await, Err(StatusCode::UNAUTHORIZED));
    assert!(
        tokens
            .list_for_user("bob")
            .await
            .unwrap()
            .is_empty(),
    );
}
