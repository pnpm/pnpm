//! The SCIM endpoints (`/-/pnpr/v0/scim/v2`) with `auth.scim`: who may call
//! them, the `User` resource, and what deprovisioning takes away.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::{
    AuthState, Config, TokenBackend, TokenStore, UpsertOutcome, UserBackend, UserStore, router,
    router_with_auth,
};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::Path,
    sync::{Arc, OnceLock},
};
use tempfile::TempDir;
use tower::ServiceExt;

const SCIM_TOKEN: &str = "scim-secret-0123456789abcdef0123456789";

const CONFIG: &str = "\
auth:
  scim:
    token: scim-secret-0123456789abcdef0123456789
  htpasswd:
    max_users: 100
";

const USERS: &str = "/-/pnpr/v0/scim/v2/Users";

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
        .header("content-type", "application/scim+json");
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

async fn scim(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    send(app, method, path, Some(SCIM_TOKEN), body).await
}

/// Log in (or register) `username`, returning the status and the token.
async fn log_in(app: &axum::Router, username: &str) -> (StatusCode, Option<String>) {
    let body = json!({ "name": username, "password": "secret", "type": "user", "roles": [] });
    let path = format!("/-/user/org.couchdb.user:{username}");
    let (status, payload) = send(app, "PUT", &path, None, Some(body)).await;
    (status, payload["token"].as_str().map(str::to_string))
}

async fn whoami(app: &axum::Router, token: &str) -> StatusCode {
    send(app, "GET", "/-/whoami", Some(token), None).await.0
}

fn new_user(username: &str) -> Value {
    json!({
        "schemas": ["urn:ietf:params:scim:schemas:core:2.0:User"],
        "userName": username,
        "externalId": "00u1",
        "name": { "givenName": "Alice" },
        "active": true,
    })
}

fn deactivate() -> Value {
    json!({
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
        "Operations": [{ "op": "replace", "path": "active", "value": "False" }],
    })
}

#[tokio::test]
async fn only_the_scim_token_reaches_the_endpoints() {
    let dir = TempDir::new().unwrap();
    let app = router(load_config(dir.path()));
    let (_, user_token) = log_in(&app, "alice").await;
    for token in [None, user_token.as_deref(), Some("scim-secret")] {
        let (status, error) = send(&app, "GET", USERS, token, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(error["status"], json!("401"));
    }
    let (status, config) =
        scim(&app, "GET", "/-/pnpr/v0/scim/v2/ServiceProviderConfig", None).await;
    assert_eq!((status, &config["patch"]["supported"]), (StatusCode::OK, &json!(true)));
}

#[tokio::test]
async fn users_are_created_found_and_patched() {
    let dir = TempDir::new().unwrap();
    let app = router(load_config(dir.path()));
    let (status, created) = scim(&app, "POST", USERS, Some(new_user("alice"))).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!((&created["id"], &created["active"]), (&json!("alice"), &json!(true)));
    assert_eq!(
        created["meta"]["location"],
        json!("http://example.test/-/pnpr/v0/scim/v2/Users/alice"),
    );
    let (status, error) = scim(&app, "POST", USERS, Some(new_user("alice"))).await;
    assert_eq!((status, &error["scimType"]), (StatusCode::CONFLICT, &json!("uniqueness")));

    let filter = format!("{USERS}?filter=userName%20eq%20%22alice%22");
    let (_, found) = scim(&app, "GET", &filter, None).await;
    assert_eq!(found["totalResults"], json!(1));
    assert_eq!(found["Resources"][0]["name"]["givenName"], json!("Alice"));
    let (_, missing) = scim(&app, "GET", &filter.replace("alice", "bob"), None).await;
    assert_eq!(missing["totalResults"], json!(0));

    let rename = json!({
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
        "Operations": [
            { "op": "replace", "path": "name.givenName", "value": "Alicia" },
            { "op": "replace", "path": r#"emails[type eq "work"].value"#, "value": "a@example.test" },
        ],
    });
    let (status, patched) = scim(&app, "PATCH", &format!("{USERS}/alice"), Some(rename)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (&patched["name"]["givenName"], &patched["active"]),
        (&json!("Alicia"), &json!(true)),
    );
}

#[tokio::test]
async fn deactivating_a_user_revokes_its_tokens_and_refuses_its_login() {
    let dir = TempDir::new().unwrap();
    let app = router(load_config(dir.path()));
    let (_, token) = log_in(&app, "alice").await;
    let token = token.unwrap();
    assert_eq!(scim(&app, "POST", USERS, Some(new_user("alice"))).await.0, StatusCode::CREATED);
    assert_eq!(whoami(&app, &token).await, StatusCode::OK);

    let (status, user) = scim(&app, "PATCH", &format!("{USERS}/alice"), Some(deactivate())).await;
    assert_eq!((status, &user["active"]), (StatusCode::OK, &json!(false)));
    assert_eq!(whoami(&app, &token).await, StatusCode::UNAUTHORIZED);
    assert_eq!(log_in(&app, "alice").await.0, StatusCode::FORBIDDEN);

    let reactivate = json!({
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
        "Operations": [{ "op": "replace", "value": { "active": true } }],
    });
    assert_eq!(
        scim(&app, "PATCH", &format!("{USERS}/alice"), Some(reactivate)).await.0,
        StatusCode::OK,
    );
    let (status, token) = log_in(&app, "alice").await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(whoami(&app, &token.unwrap()).await, StatusCode::OK);
}

#[tokio::test]
async fn a_deleted_user_stays_refused_and_unlisted_until_provisioned_again() {
    let dir = TempDir::new().unwrap();
    let app = router(load_config(dir.path()));
    assert_eq!(scim(&app, "POST", USERS, Some(new_user("alice"))).await.0, StatusCode::CREATED);
    let path = format!("{USERS}/alice");
    assert_eq!(scim(&app, "DELETE", &path, None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(scim(&app, "GET", &path, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(scim(&app, "DELETE", &path, None).await.0, StatusCode::NOT_FOUND);
    let (_, listed) = scim(&app, "GET", USERS, None).await;
    assert_eq!(listed["totalResults"], json!(0));

    let restarted = router(load_config(dir.path()));
    assert_eq!(log_in(&restarted, "alice").await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        scim(&restarted, "POST", USERS, Some(new_user("alice"))).await.0,
        StatusCode::CREATED,
    );
    assert_eq!(log_in(&restarted, "alice").await.0, StatusCode::CREATED);
}

#[tokio::test]
async fn without_auth_scim_the_endpoints_are_not_served() {
    let dir = TempDir::new().unwrap();
    let storage = dir.path().join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    let path = dir.path().join("config.yaml");
    std::fs::write(&path, format!("storage: {}\n", storage.display())).unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let app = router(Config::from_yaml(&path, listen, None).unwrap());
    let (status, _) = scim(&app, "GET", USERS, None).await;
    assert!(status == StatusCode::NOT_FOUND || status == StatusCode::BAD_GATEWAY, "{status}");
}

#[tokio::test]
async fn reactivating_a_user_first_removes_credentials_a_failed_cleanup_left() {
    let dir = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let app = router_with_auth(load_config(dir.path()), auth.clone());
    assert_eq!(scim(&app, "POST", USERS, Some(new_user("alice"))).await.0, StatusCode::CREATED);
    assert_eq!(
        scim(&app, "PATCH", &format!("{USERS}/alice"), Some(deactivate())).await.0,
        StatusCode::OK,
    );
    // What a cleanup that failed while alice was inactive would leave behind.
    auth.users.create_user("alice", "secret").await.unwrap();
    let left_behind = auth.tokens.issue("alice").await.unwrap();

    let reactivate = json!({
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
        "Operations": [{ "op": "replace", "path": "active", "value": true }],
    });
    assert_eq!(
        scim(&app, "PATCH", &format!("{USERS}/alice"), Some(reactivate)).await.0,
        StatusCode::OK,
    );
    assert_eq!(whoami(&app, &left_behind).await, StatusCode::UNAUTHORIZED);
    assert!(
        auth.users
            .password_hash("alice")
            .await
            .unwrap()
            .is_none(),
    );
}

/// A user store whose registration lets a SCIM client deactivate the user
/// first, as if the deactivation landed while the login ran.
struct DeactivatedDuringLogin {
    users: UserStore,
    app: OnceLock<axum::Router>,
}

#[async_trait::async_trait]
impl UserBackend for DeactivatedDuringLogin {
    async fn add_or_login(
        &self,
        username: &str,
        password: &str,
    ) -> pnpr::Result<(UpsertOutcome, String)> {
        let app = self.app.get().expect("router set");
        let (status, _) =
            scim(app, "PATCH", &format!("{USERS}/{username}"), Some(deactivate())).await;
        assert_eq!(status, StatusCode::OK);
        self.users.add_or_login(username, password).await
    }

    async fn list_users(&self) -> pnpr::Result<Vec<String>> {
        self.users.list_users().await
    }

    async fn password_hash(&self, username: &str) -> pnpr::Result<Option<String>> {
        self.users.password_hash(username).await
    }

    async fn create_user(&self, username: &str, password: &str) -> pnpr::Result<bool> {
        self.users.create_user(username, password).await
    }

    async fn set_password(&self, username: &str, password: &str) -> pnpr::Result<bool> {
        self.users.set_password(username, password).await
    }

    async fn delete_user(&self, username: &str) -> pnpr::Result<bool> {
        self.users.delete_user(username).await
    }
}

#[tokio::test]
async fn a_login_racing_a_deactivation_keeps_no_token_or_account() {
    let dir = TempDir::new().unwrap();
    let users =
        Arc::new(DeactivatedDuringLogin { users: UserStore::in_memory(), app: OnceLock::new() });
    let tokens = Arc::new(TokenStore::in_memory());
    let auth = AuthState { users: Arc::clone(&users) as _, tokens: Arc::clone(&tokens) as _ };
    let app = router_with_auth(load_config(dir.path()), auth);
    users.app.set(app.clone()).unwrap();
    assert_eq!(scim(&app, "POST", USERS, Some(new_user("alice"))).await.0, StatusCode::CREATED);

    assert_eq!(log_in(&app, "alice").await.0, StatusCode::FORBIDDEN);
    assert!(
        tokens
            .list_for_user("alice")
            .await
            .unwrap()
            .is_empty(),
    );
    assert!(
        users.users
            .password_hash("alice")
            .await
            .unwrap()
            .is_none(),
    );
}
