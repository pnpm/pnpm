//! The SCIM endpoints (`/-/pnpr/v0/scim/v2`) with `auth.scim`: who may call
//! them, the `User` resource, and what deprovisioning takes away.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::{AuthState, Config, router, router_with_auth};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::Path,
};
use tempfile::TempDir;
use tower::ServiceExt;

const SCIM_TOKEN: &str = "scim-secret-0123456789abcdef0123456789";

const CONFIG: &str = "\
auth:
  scim:
    token: scim-secret-0123456789abcdef0123456789
  users:
    maxUsers: 100
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
    let body = json!({ "name": username, "password": "secret123", "type": "user", "roles": [] });
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
async fn users_have_stable_ids_and_mutable_login_names() {
    let dir = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let app = router_with_auth(load_config(dir.path()), auth.clone());
    let (status, created) = scim(&app, "POST", USERS, Some(new_user("alice"))).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap();
    assert_ne!(id, "alice");
    assert_eq!(
        auth.users
            .account_id("alice")
            .await
            .unwrap()
            .as_deref(),
        Some(id),
    );
    let path = format!("{USERS}/{id}");
    let (_, listed) =
        scim(&app, "GET", &format!("{USERS}?filter=userName%20eq%20%22alice%22"), None).await;
    assert_eq!(listed["Resources"][0]["id"], id);
    let (status, replaced) = scim(&app, "PUT", &path, Some(new_user("renamed"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replaced["id"], id);
    assert_eq!(replaced["userName"], "renamed");
    assert_eq!(
        auth.users
            .account_name(id)
            .await
            .unwrap()
            .as_deref(),
        Some("renamed"),
    );
    let patch = json!({"schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"], "Operations": [{"op": "replace", "path": "userName", "value": "patched"}]});
    let (status, patched) = scim(&app, "PATCH", &path, Some(patch)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(patched["id"], id);
    assert_eq!(
        auth.users
            .account_name(id)
            .await
            .unwrap()
            .as_deref(),
        Some("patched"),
    );
}

#[tokio::test]
async fn deactivation_revokes_credentials_and_reactivation_keeps_the_id() {
    let dir = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let app = router_with_auth(load_config(dir.path()), auth.clone());
    let (_, created) = scim(&app, "POST", USERS, Some(new_user("alice"))).await;
    let id = created["id"].as_str().unwrap();
    auth.users.set_password(id, "secret123").await.unwrap();
    let (_, token) = log_in(&app, "alice").await;
    let token = token.unwrap();
    let path = format!("{USERS}/{id}");
    assert_eq!(scim(&app, "PATCH", &path, Some(deactivate())).await.0, StatusCode::OK);
    assert_eq!(whoami(&app, &token).await, StatusCode::UNAUTHORIZED);
    assert_eq!(log_in(&app, "alice").await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        auth.users
            .account_id("alice")
            .await
            .unwrap()
            .as_deref(),
        Some(id),
    );
    let (_, reactivated) = scim(&app, "PUT", &path, Some(new_user("alice"))).await;
    assert_eq!(reactivated["id"], id);
    assert_eq!(whoami(&app, &token).await, StatusCode::UNAUTHORIZED);
    assert_eq!(log_in(&app, "alice").await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        auth.users
            .password_hash("alice")
            .await
            .unwrap()
            .as_deref(),
        Some(pnpr_auth::NO_PASSWORD_HASH),
    );
    let signed_in = auth.tokens.issue(id).await.unwrap();
    let (status, created_token) =
        send(&app, "POST", "/-/npm/v1/tokens", Some(&signed_in), Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(created_token.get("expires").is_none());
    assert_eq!(whoami(&app, created_token["token"].as_str().unwrap()).await, StatusCode::OK);
    auth.users.set_password(id, "secret123").await.unwrap();
    assert_eq!(log_in(&app, "alice").await.0, StatusCode::CREATED);
}

#[tokio::test]
async fn deleting_and_reprovisioning_a_name_never_reassigns_its_id() {
    let dir = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let app = router_with_auth(load_config(dir.path()), auth.clone());
    let (_, created) = scim(&app, "POST", USERS, Some(new_user("alice"))).await;
    let id = created["id"].as_str().unwrap();
    let old_token = auth.tokens.issue(id).await.unwrap();
    let path = format!("{USERS}/{id}");
    assert_eq!(scim(&app, "DELETE", &path, None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(scim(&app, "GET", &path, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(scim(&app, "PUT", &path, Some(new_user("alice"))).await.0, StatusCode::NOT_FOUND);
    let restarted = router_with_auth(load_config(dir.path()), auth.clone());
    let (status, replacement) = scim(&restarted, "POST", USERS, Some(new_user("alice"))).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_ne!(replacement["id"], id);
    assert_eq!(whoami(&restarted, &old_token).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn provisioning_cannot_silently_link_an_existing_password_account() {
    let dir = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let app = router_with_auth(load_config(dir.path()), auth.clone());
    let (_, token) = log_in(&app, "alice").await;
    let original = auth.users
        .account_id("alice")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(scim(&app, "POST", USERS, Some(new_user("alice"))).await.0, StatusCode::CONFLICT);
    assert_eq!(
        auth.users
            .account_id("alice")
            .await
            .unwrap()
            .as_deref(),
        Some(original.as_str()),
    );
    assert_eq!(whoami(&app, &token.unwrap()).await, StatusCode::OK);
    let (_, created) = scim(&app, "POST", USERS, Some(new_user("bob"))).await;
    assert_eq!(log_in(&app, "bob").await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        auth.users
            .account_id("bob")
            .await
            .unwrap()
            .as_deref(),
        created["id"].as_str(),
    );
}

#[tokio::test]
async fn reactivation_cleans_up_credentials_left_during_deactivation() {
    let dir = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let app = router_with_auth(load_config(dir.path()), auth.clone());
    let (_, created) = scim(&app, "POST", USERS, Some(new_user("alice"))).await;
    let id = created["id"].as_str().unwrap();
    let path = format!("{USERS}/{id}");
    scim(&app, "PATCH", &path, Some(deactivate())).await;
    auth.users.set_password(id, "secret123").await.unwrap();
    let leftover = auth.tokens.issue(id).await.unwrap();
    assert_eq!(scim(&app, "PUT", &path, Some(new_user("alice"))).await.0, StatusCode::OK);
    assert_eq!(whoami(&app, &leftover).await, StatusCode::UNAUTHORIZED);
    assert_eq!(log_in(&app, "alice").await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_unreadable_directory_refuses_user_credentials_until_it_reads() {
    let dir = TempDir::new().unwrap();
    let config = load_config(dir.path());
    let _storage = pnpr_storage::Storage::from_config(&config.storage).unwrap();
    let pnpr_config::MetadataBackend::Sqlite(sqlite_path) = &config.storage.metadata else {
        unreachable!();
    };
    let conn = rusqlite::Connection::open(sqlite_path).unwrap();
    conn.execute(
        "INSERT INTO scim_users (user_id, active, removed, deprovisions, attributes, generation)
         VALUES ('corrupt-user', 2, 0, 0, '{}', 1)",
        [],
    )
    .unwrap();
    drop(conn);
    let auth = AuthState::in_memory();
    auth.users.create_user("alice", "secret123").await.unwrap();
    let id = auth.users
        .account_id("alice")
        .await
        .unwrap()
        .unwrap();
    let token = auth.tokens.issue(&id).await.unwrap();
    let app = router_with_auth(load_config(dir.path()), auth);

    assert_eq!(whoami(&app, &token).await, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(log_in(&app, "alice").await.0, StatusCode::INTERNAL_SERVER_ERROR);

    let conn = rusqlite::Connection::open(sqlite_path).unwrap();
    conn.execute("DELETE FROM scim_users WHERE user_id = 'corrupt-user'", [])
        .unwrap();
    drop(conn);
    // A failed read is retried a second later, in the background of the
    // first request after that.
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let mut status = whoami(&app, &token).await;
    for _ in 0..100 {
        if status == StatusCode::OK {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        status = whoami(&app, &token).await;
    }
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn deleting_retries_cleanup_after_the_resource_was_marked_removed() {
    let dir = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let app = router_with_auth(load_config(dir.path()), auth.clone());
    let (_, created) = scim(&app, "POST", USERS, Some(new_user("alice"))).await;
    let id = created["id"].as_str().unwrap();
    let token = auth.tokens.issue(id).await.unwrap();
    let storage = pnpr_storage::Storage::from_config(&load_config(dir.path()).storage).unwrap();
    assert!(matches!(
        storage.remove_scim_user(id).await.unwrap(),
        pnpr_storage::metadata::ScimUserRemoval::Removed(_),
    ));
    assert_eq!(
        scim(&app, "DELETE", &format!("{USERS}/{id}"), None).await.0,
        StatusCode::NO_CONTENT,
    );
    assert_eq!(auth.users.account_name(id).await.unwrap(), None);
    assert_eq!(whoami(&app, &token).await, StatusCode::UNAUTHORIZED);
    assert_eq!(scim(&app, "POST", USERS, Some(new_user("alice"))).await.0, StatusCode::CREATED);
}

#[tokio::test]
async fn deactivating_and_deleting_complete_cleanup_when_directory_reload_fails() {
    let dir = TempDir::new().unwrap();
    let config = load_config(dir.path());
    let pnpr_config::MetadataBackend::Sqlite(sqlite_path) = config.storage
        .metadata
        .clone()
    else {
        unreachable!();
    };
    let auth = AuthState::in_memory();
    let app = router_with_auth(config, auth.clone());

    let (_, alice) = scim(&app, "POST", USERS, Some(new_user("alice"))).await;
    let alice_id = alice["id"].as_str().unwrap();
    let alice_token = auth.tokens.issue(alice_id).await.unwrap();

    let (_, bob) = scim(&app, "POST", USERS, Some(new_user("bob"))).await;
    let bob_id = bob["id"].as_str().unwrap();
    let bob_token = auth.tokens.issue(bob_id).await.unwrap();

    let (_, carol) = scim(&app, "POST", USERS, Some(new_user("carol"))).await;
    let carol_id = carol["id"].as_str().unwrap();
    let carol_token = auth.tokens.issue(carol_id).await.unwrap();

    let conn = rusqlite::Connection::open(&sqlite_path).unwrap();
    conn.execute(
        "INSERT INTO scim_users (user_id, active, removed, deprovisions, attributes, generation)
         VALUES ('corrupt-user', 2, 0, 0, '{}', 100)",
        [],
    )
    .unwrap();
    drop(conn);

    assert_eq!(
        scim(&app, "PATCH", &format!("{USERS}/{alice_id}"), Some(deactivate()))
            .await
            .0,
        StatusCode::OK,
    );
    assert_eq!(whoami(&app, &alice_token).await, StatusCode::UNAUTHORIZED);
    assert_eq!(whoami(&app, &carol_token).await, StatusCode::INTERNAL_SERVER_ERROR);

    assert_eq!(
        scim(&app, "DELETE", &format!("{USERS}/{bob_id}"), None).await.0,
        StatusCode::NO_CONTENT,
    );
    assert_eq!(auth.users.account_name(bob_id).await.unwrap(), None);
    assert_eq!(whoami(&app, &bob_token).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn patch_rolls_back_username_when_scim_row_fails_to_commit() {
    let dir = TempDir::new().unwrap();
    let config = load_config(dir.path());
    let pnpr_config::MetadataBackend::Sqlite(sqlite_path) = config.storage
        .metadata
        .clone()
    else {
        unreachable!();
    };
    let auth = AuthState::in_memory();
    let app = router_with_auth(config, auth.clone());

    let (_, alice) = scim(&app, "POST", USERS, Some(new_user("alice"))).await;
    let alice_id = alice["id"].as_str().unwrap();

    let conn = rusqlite::Connection::open(&sqlite_path).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER fail_scim_update BEFORE UPDATE ON scim_users
         BEGIN SELECT RAISE(ABORT, 'simulated failure'); END;",
    )
    .unwrap();
    drop(conn);

    let rename_patch = serde_json::json!({
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
        "Operations": [{ "op": "replace", "path": "userName", "value": "alice-renamed" }],
    });
    assert_eq!(
        scim(&app, "PATCH", &format!("{USERS}/{alice_id}"), Some(rename_patch))
            .await
            .0,
        StatusCode::INTERNAL_SERVER_ERROR,
    );
    assert_eq!(
        auth.users
            .account_name(alice_id)
            .await
            .unwrap()
            .as_deref(),
        Some("alice"),
    );
}

#[tokio::test]
async fn failed_scim_rollback_never_undoes_a_newer_claimed_login_name() {
    let dir = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let app = router_with_auth(load_config(dir.path()), auth.clone());

    let (_, alice) = scim(&app, "POST", USERS, Some(new_user("alice"))).await;
    let alice_id = alice["id"].as_str().unwrap();

    // Replica A claims "bob" at revision 1, then Replica B accepts PUT("bob")
    // at revision 2 before A's stale rollback runs.
    let initial = auth.users
        .account_login(alice_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        auth.users.rename_user_if_current(alice_id, initial.revision, "bob").await.unwrap(),
        pnpr_auth::UserRename::Renamed(1),
    );
    assert_eq!(
        scim(&app, "PUT", &format!("{USERS}/{alice_id}"), Some(new_user("bob")))
            .await
            .0,
        StatusCode::OK,
    );
    assert_eq!(
        auth.users.rename_user_if_current(alice_id, 1, "alice").await.unwrap(),
        pnpr_auth::UserRename::CurrentChanged,
    );
    assert_eq!(
        auth.users
            .account_name(alice_id)
            .await
            .unwrap()
            .as_deref(),
        Some("bob"),
    );

    // An intervening admin rename also advances the revision and blocks stale rollback.
    let current = auth.users
        .account_login(alice_id)
        .await
        .unwrap()
        .unwrap();
    let pnpr_auth::UserRename::Renamed(claimed_rev) =
        auth.users.rename_user_if_current(alice_id, current.revision, "carol").await.unwrap()
    else {
        unreachable!();
    };
    auth.users.rename_user(alice_id, "carol").await.unwrap();
    assert_eq!(
        auth.users.rename_user_if_current(alice_id, claimed_rev, "bob").await.unwrap(),
        pnpr_auth::UserRename::CurrentChanged,
    );
}
