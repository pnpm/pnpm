//! The admin API for accounts (`/-/pnpr/v0/admin/users`): who may use it, and
//! what creating, re-keying, and removing an account does to logins and
//! tokens.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::{
    AuthState, Config, OwnedTokens, PasswordChange, RegistryError, TokenBackend, TokenRecord,
    TokenStore, UpsertOutcome, UserBackend, UserCreation, UserRemoval, UserStore,
};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::Arc,
};
use tempfile::TempDir;
use tower::ServiceExt;

/// The admin `root` is provisioned ahead of time, with `max_users`: 1 so
/// every other account has to come from the admin API.
fn load_config(dir: &TempDir) -> Config {
    let storage = dir.path().join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    let yaml = format!(
        "storage: {}\nauth:\n  admins: [root]\n  users:\n    file: ./htpasswd\n    maxUsers: 1\n",
        storage.display(),
    );
    let path = dir.path().join("config.yaml");
    std::fs::write(&path, yaml).unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    Config::from_yaml(&path, listen, Some("http://example.test".to_string())).unwrap()
}

async fn app_router(config: Config) -> axum::Router {
    let auth = crate::tokens::seeded_auth(&config, &[("root", "secret")]);
    account_router(config, auth).await
}

async fn send(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let path = crate::tokens::admin_path(app, method, path).await;
    let mut request = Request::builder()
        .method(method)
        .uri(&path)
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
    let app = app_router(load_config(&dir)).await;
    let root = login(&app, "root", "secret").await.unwrap();
    assert_eq!(login(&app, "bob", "first").await, Err(StatusCode::FORBIDDEN));

    let body = json!({ "password": "first" });
    let (status, _) =
        send(&app, "PUT", "/-/pnpr/v0/admin/users/bob", Some(&root), Some(body)).await;
    assert_eq!(status, StatusCode::CREATED);
    let bob = login(&app, "bob", "first").await.unwrap();
    assert_eq!(whoami(&app, &bob).await, StatusCode::OK);

    let (_, users) = send(&app, "GET", "/-/pnpr/v0/admin/users", Some(&root), None).await;
    assert_eq!(
        users["users"]
            .as_array()
            .unwrap()
            .iter()
            .map(|user| user["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["bob", "root"],
    );
}

#[tokio::test]
async fn a_new_password_replaces_the_old_one() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(&dir)).await;
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
    let app = app_router(load_config(&dir)).await;
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
    let app = app_router(load_config(&dir)).await;
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
    let app = app_router(load_config(&dir)).await;
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

    async fn create_user(&self, _username: &str, _password: &str) -> pnpr::Result<UserCreation> {
        Ok(UserCreation::NameTaken)
    }

    async fn set_password(&self, _username: &str, _password: &str) -> pnpr::Result<PasswordChange> {
        Ok(PasswordChange::NoSuchUser)
    }

    async fn clear_password(&self, _user_id: &str) -> pnpr::Result<PasswordChange> {
        Ok(PasswordChange::NoSuchUser)
    }

    async fn delete_user(&self, _username: &str) -> pnpr::Result<UserRemoval> {
        Ok(UserRemoval::NoSuchUser)
    }

    async fn account_id(&self, _name: &str) -> pnpr::Result<Option<String>> {
        Ok(None)
    }
    async fn account_name(&self, _id: &str) -> pnpr::Result<Option<String>> {
        Ok(None)
    }
    async fn rename_user(&self, _id: &str, _name: &str) -> pnpr::Result<()> {
        unreachable!()
    }
    async fn provision_user(&self, _name: &str) -> pnpr::Result<String> {
        unreachable!()
    }
}

#[tokio::test]
async fn a_login_racing_a_removal_keeps_no_token() {
    let dir = TempDir::new().unwrap();
    let tokens = Arc::new(TokenStore::in_memory());
    let auth = AuthState { users: Arc::new(RemovedDuringLogin), tokens: Arc::clone(&tokens) as _ };
    let app = account_router(load_config(&dir), auth).await;

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
    async fn issue_token(
        &self,
        username: &str,
        new_token: pnpr_auth::NewToken,
    ) -> pnpr::Result<(String, TokenRecord)> {
        self.0.issue_token(username, new_token).await
    }

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
async fn a_failed_revocation_still_disables_the_account() {
    let dir = TempDir::new().unwrap();
    let users = Arc::new(UserStore::in_memory());
    users.create_user("root", "secret").await.unwrap();
    let auth = AuthState {
        users: Arc::clone(&users) as _,
        tokens: Arc::new(FailingRevocation(TokenStore::in_memory())),
    };
    let app = account_router(load_config(&dir), auth).await;
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
            .is_none(),
    );
}

/// A user store whose one account is removed and created again between a
/// login's password check and its token: the hash the login verified is
/// followed by another.
struct ReplacedDuringLogin {
    outcome: UpsertOutcome,
    /// Popped by each `password_hash` call: the hash the login sees before
    /// verifying, then the one it sees once its token exists.
    hashes: std::sync::Mutex<Vec<Option<String>>>,
}

#[async_trait::async_trait]
impl UserBackend for ReplacedDuringLogin {
    async fn add_or_login(
        &self,
        username: &str,
        _password: &str,
    ) -> pnpr::Result<(UpsertOutcome, String)> {
        Ok((self.outcome, username.to_string()))
    }

    async fn list_users(&self) -> pnpr::Result<Vec<String>> {
        Ok(Vec::new())
    }

    async fn password_hash(&self, _username: &str) -> pnpr::Result<Option<String>> {
        Ok(self.hashes
            .lock()
            .unwrap()
            .pop()
            .flatten())
    }

    async fn create_user(&self, _username: &str, _password: &str) -> pnpr::Result<UserCreation> {
        Ok(UserCreation::NameTaken)
    }

    async fn set_password(&self, _username: &str, _password: &str) -> pnpr::Result<PasswordChange> {
        Ok(PasswordChange::NoSuchUser)
    }

    async fn clear_password(&self, _user_id: &str) -> pnpr::Result<PasswordChange> {
        Ok(PasswordChange::NoSuchUser)
    }

    async fn delete_user(&self, _username: &str) -> pnpr::Result<UserRemoval> {
        Ok(UserRemoval::NoSuchUser)
    }

    async fn account_id(&self, _name: &str) -> pnpr::Result<Option<String>> {
        Ok(None)
    }
    async fn account_name(&self, _id: &str) -> pnpr::Result<Option<String>> {
        Ok(None)
    }
    async fn rename_user(&self, _id: &str, _name: &str) -> pnpr::Result<()> {
        unreachable!()
    }
    async fn provision_user(&self, _name: &str) -> pnpr::Result<String> {
        unreachable!()
    }
}

#[tokio::test]
async fn an_old_password_gets_no_token_for_a_recreated_account() {
    let dir = TempDir::new().unwrap();
    let tokens = Arc::new(TokenStore::in_memory());
    let hashes = vec![Some("new hash".to_string()), Some("old hash".to_string())];
    let users = ReplacedDuringLogin {
        outcome: UpsertOutcome::LoggedIn,
        hashes: std::sync::Mutex::new(hashes),
    };
    let auth = AuthState { users: Arc::new(users), tokens: Arc::clone(&tokens) as _ };
    let app = account_router(load_config(&dir), auth).await;

    assert_eq!(login(&app, "bob", "old").await, Err(StatusCode::UNAUTHORIZED));
    assert!(
        tokens
            .list_for_user("bob")
            .await
            .unwrap()
            .is_empty(),
    );
}

#[tokio::test]
async fn a_registration_gets_no_token_for_an_account_recreated_with_another_password() {
    let dir = TempDir::new().unwrap();
    let tokens = Arc::new(TokenStore::in_memory());
    let replacement = bcrypt::hash("admin's password", 4).unwrap();
    let users = ReplacedDuringLogin {
        outcome: UpsertOutcome::Created,
        hashes: std::sync::Mutex::new(vec![Some(replacement), None]),
    };
    let auth = AuthState { users: Arc::new(users), tokens: Arc::clone(&tokens) as _ };
    let app = account_router(load_config(&dir), auth).await;

    assert_eq!(login(&app, "bob", "mine").await, Err(StatusCode::UNAUTHORIZED));
    assert!(
        tokens
            .list_for_user("bob")
            .await
            .unwrap()
            .is_empty(),
    );
}

#[tokio::test]
async fn a_removed_account_loses_its_tokens_without_any_revocation() {
    let dir = TempDir::new().unwrap();
    let users = Arc::new(UserStore::in_memory());
    let tokens = OwnedTokens::new(TokenStore::in_memory(), Arc::clone(&users));
    let auth = AuthState { users: Arc::clone(&users) as _, tokens: Arc::new(tokens) };
    let app = account_router(load_config(&dir), auth).await;
    users.create_user("bob", "x").await.unwrap();
    let bob = login(&app, "bob", "x").await.unwrap();
    assert_eq!(whoami(&app, &bob).await, StatusCode::OK);

    users
        .delete_user(
            &users
                .account_id("bob")
                .await
                .unwrap()
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(whoami(&app, &bob).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_recreated_account_does_not_inherit_tokens_left_behind() {
    let dir = TempDir::new().unwrap();
    let users = Arc::new(UserStore::in_memory());
    users.create_user("root", "secret").await.unwrap();
    let tokens = Arc::new(OwnedTokens::new(TokenStore::in_memory(), Arc::clone(&users)));
    let auth = AuthState { users: Arc::clone(&users) as _, tokens: Arc::clone(&tokens) as _ };
    let app = account_router(load_config(&dir), auth).await;
    let root = login(&app, "root", "secret").await.unwrap();
    let mut left_behind = Vec::new();
    for name in ["bob", "carol"] {
        users.create_user(name, "old").await.unwrap();
        let id = users
            .account_id(name)
            .await
            .unwrap()
            .unwrap();
        left_behind.push(tokens.issue(&id).await.unwrap());
        users.delete_user(&id).await.unwrap();
    }

    let body = json!({ "password": "new" });
    let (status, _) =
        send(&app, "PUT", "/-/pnpr/v0/admin/users/bob", Some(&root), Some(body)).await;
    assert_eq!(status, StatusCode::CREATED);
    let carol = login(&app, "carol", "new").await.unwrap();
    for token in &left_behind {
        assert_eq!(whoami(&app, token).await, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(whoami(&app, &carol).await, StatusCode::OK);
}

#[tokio::test]
async fn deleted_names_can_be_reclaimed_without_reviving_tokens() {
    let dir = TempDir::new().unwrap();
    let config = load_config(&dir);
    let auth = AuthState::in_memory();
    auth.users.create_user("root", "secret").await.unwrap();
    let app = account_router(config, auth).await;
    let root = login(&app, "root", "secret").await.unwrap();
    let previous = login(&app, "former-member", "old-password").await.unwrap();
    let path = "/-/pnpr/v0/admin/users/former-member";
    assert_eq!(send(&app, "DELETE", path, Some(&root), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(whoami(&app, &previous).await, StatusCode::UNAUTHORIZED);
    assert!(login(&app, "former-member", "new-password").await.is_ok());
    assert_eq!(whoami(&app, &previous).await, StatusCode::UNAUTHORIZED);
    assert!(login(&app, "new-member", "password").await.is_ok());
}

#[tokio::test]
async fn deleting_orphan_tokens_does_not_reserve_a_login_name() {
    let dir = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    auth.users.create_user("root", "secret").await.unwrap();
    auth.tokens.issue("oidc-member").await.unwrap();
    let users = Arc::clone(&auth.users);
    let app = account_router(load_config(&dir), auth).await;
    let root = login(&app, "root", "secret").await.unwrap();
    assert!(
        users
            .password_hash("oidc-member")
            .await
            .unwrap()
            .is_none(),
    );
    assert_eq!(
        send(&app, "DELETE", "/-/pnpr/v0/admin/users/oidc-member", Some(&root), None)
            .await
            .0,
        StatusCode::NO_CONTENT,
    );
    assert_eq!(users.account_id("oidc-member").await.unwrap(), None);
    assert!(login(&app, "oidc-member", "new-password").await.is_ok());
    assert_eq!(
        send(&app, "DELETE", "/-/pnpr/v0/admin/users/unknown-member", Some(&root), None)
            .await
            .0,
        StatusCode::NOT_FOUND,
    );
    assert_eq!(users.account_id("unknown-member").await.unwrap(), None);
    assert!(login(&app, "unknown-member", "password").await.is_ok());
}

#[tokio::test]
async fn deleting_a_passwordless_oidc_account_frees_its_name_but_not_its_id() {
    let dir = TempDir::new().unwrap();
    let mut config = load_config(&dir);
    config.http.public_url = "https://example.test".to_string();
    config.identity.auth.oidc = serde_json::from_value(json!([{
        "name": "company", "issuer": "https://issuer.example.test", "audience": "pnpr",
        "login": { "users": [{ "subject": "subject-1", "userId": "oidc-member" }] },
    }]))
    .unwrap();
    let auth = AuthState::in_memory();
    auth.users.create_user("root", "secret").await.unwrap();
    let users = Arc::clone(&auth.users);
    let id = users.provision_user("oidc-member").await.unwrap();
    config.identity.auth.oidc[0].login.as_mut().unwrap().users[0].user_id = id.clone();
    let app = account_router(config, auth).await;
    let root = login(&app, "root", "secret").await.unwrap();
    assert_eq!(
        send(&app, "DELETE", "/-/pnpr/v0/admin/users/oidc-member", Some(&root), None)
            .await
            .0,
        StatusCode::NO_CONTENT,
    );
    assert_eq!(users.account_name(&id).await.unwrap(), None);
    assert_eq!(
        send(
            &app,
            "PUT",
            "/-/pnpr/v0/admin/users/oidc-member",
            Some(&root),
            Some(json!({"password":"new-password"}))
        )
        .await
        .0,
        StatusCode::CREATED,
    );
}

async fn account_router(mut config: Config, auth: AuthState) -> axum::Router {
    if let Some(root_id) = auth.users.account_id("root").await.unwrap() {
        config.identity.admins = [root_id].into();
    }
    pnpr::router_with_auth(config, auth)
}

#[tokio::test]
async fn invalid_rename_payloads_are_client_errors() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(&dir)).await;
    let root = login(&app, "root", "secret").await.unwrap();
    let id = crate::tokens::account_id(&app, "root").await.unwrap();
    let path = format!("/-/pnpr/v0/admin/users/{id}");
    for payload in [json!({"name": 42}), json!({"name": "renamed", "extra": true})] {
        assert_eq!(
            send(&app, "PATCH", &path, Some(&root), Some(payload)).await.0,
            StatusCode::BAD_REQUEST,
        );
    }
    assert_eq!(crate::tokens::account_id(&app, "root").await, Some(id));
}
