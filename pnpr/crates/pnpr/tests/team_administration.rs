//! Team administration through the npm team API on registries the metadata
//! database stores (`registriesManagedBy: api`): who may edit a roster, how an
//! edit reaches the `team:` rules, and that the roster persists in the metadata
//! database.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::{Config, router_with_auth};
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
  users:
    maxUsers: 100
registries:
  local:
    type: hosted
    namespace: \"\"
    teams:
      readers: { name: readers, members: [] }
    packages:
      '@secret/*':
        access: [team:readers]
      '**': {}
  main:
    type: router
    sources: [local]
defaultRegistry: main
";

async fn load_config(dir: &Path) -> Config {
    load_config_with(dir, CONFIG).await
}

async fn load_config_with(dir: &Path, yaml: &str) -> Config {
    let storage = dir.join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    let yaml = format!("storage: {}\n{yaml}", storage.display());
    crate::managed_registries::managed_config(&config_path(dir), &yaml, listen(), public_url())
        .await
}

/// The config [`load_config`] wrote, without storing its graph again.
fn reload_config(dir: &Path) -> Config {
    Config::from_yaml(&config_path(dir), listen(), public_url()).unwrap()
}

fn config_path(dir: &Path) -> std::path::PathBuf {
    dir.join("config.yaml")
}

fn listen() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873))
}

fn public_url() -> Option<String> {
    Some("http://example.test".to_string())
}

async fn app_router(config: Config) -> axum::Router {
    let auth = crate::tokens::seeded_auth(&config, &[("root", "secret123")]);
    router_with_auth(config, auth)
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

async fn add_user(app: &axum::Router, username: &str) -> String {
    let body = json!({ "name": username, "password": "secret123", "type": "user", "roles": [] });
    let path = format!("/-/user/org.couchdb.user:{username}");
    let (status, payload) = send(app, "PUT", &path, None, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED);
    payload["token"]
        .as_str()
        .expect("token in response")
        .to_string()
}

async fn team_names(app: &axum::Router, token: &str) -> Value {
    let (status, teams) = send(app, "GET", "/npm/-/org/acme/team", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    Value::Array(
        teams
            .as_array()
            .unwrap()
            .iter()
            .map(|team| json!({ "name": team["name"] }))
            .collect(),
    )
}

#[tokio::test]
async fn admin_edits_reach_team_rules_at_once() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let bob = add_user(&app, "bob").await;

    // The package does not exist, so 404 proves the read was authorized and
    // 403 proves it was not.
    let read = || send(&app, "GET", "/npm/@secret/thing", Some(&bob), None);
    assert_eq!(read().await.0, StatusCode::FORBIDDEN);

    let add = json!({ "user": "bob" });
    let path = "/npm/-/team/acme/readers/user";
    assert_eq!(send(&app, "PUT", path, Some(&root), Some(add)).await.0, StatusCode::CREATED);
    assert_eq!(read().await.0, StatusCode::NOT_FOUND);
    let (_, members) = send(&app, "GET", path, Some(&root), None).await;
    assert_eq!(members[0]["name"], "bob");
    assert_eq!(
        members[0]["id"].as_str(),
        crate::tokens::account_id(&app, "bob", Some(&root)).await.as_deref(),
    );

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
    let app = app_router(load_config(dir.path()).await).await;
    let bob = add_user(&app, "bob").await;
    let create = json!({ "name": "ops" });

    let (status, _) =
        send(&app, "PUT", "/npm/-/org/acme/team", Some(&bob), Some(create.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(&app, "PUT", "/npm/-/org/acme/team", None, Some(create)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        send(&app, "GET", "/npm/-/org/acme/team", Some(&bob), None).await.0,
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn roster_edits_refuse_conflicting_state() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let create = json!({ "name": "ops" });

    let (status, team) =
        send(&app, "PUT", "/npm/-/org/acme/team", Some(&root), Some(create.clone())).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(team["name"], "ops");
    assert!(!team["id"].as_str().unwrap().is_empty());
    let (status, _) = send(&app, "PUT", "/npm/-/org/acme/team", Some(&root), Some(create)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    // A `packages:` rule names `readers`, so it cannot be destroyed.
    let (status, _) = send(&app, "DELETE", "/npm/-/team/acme/readers", Some(&root), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = send(&app, "DELETE", "/npm/-/team/acme/nope", Some(&root), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let add = json!({ "user": "bob" });
    let (status, _) = send(&app, "PUT", "/npm/-/team/acme/nope/user", Some(&root), Some(add)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(&app, "DELETE", "/npm/-/team/acme/ops", Some(&root), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(team_names(&app, &root).await, json!([{ "name": "readers" }]));
}

#[tokio::test]
async fn a_new_process_serves_the_stored_roster() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let create = json!({ "name": "ops" });
    assert_eq!(
        send(&app, "PUT", "/npm/-/org/acme/team", Some(&root), Some(create))
            .await
            .0,
        StatusCode::CREATED,
    );

    let restarted = app_router(reload_config(dir.path())).await;
    let root = add_user(&restarted, "root").await;
    assert_eq!(
        team_names(&restarted, &root).await,
        json!([{ "name": "readers" }, { "name": "ops" }]),
    );
}

#[tokio::test]
async fn directory_reports_admin_and_registry_management() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let bob = add_user(&app, "bob").await;

    let (_, directory) = send(&app, "GET", "/-/pnpr/v0/registries", Some(&root), None).await;
    assert_eq!(directory["admin"], json!(true));
    assert_eq!(directory["registryAdmin"], json!(false));
    assert_eq!(directory["registriesManagedBy"], json!("api"));
    let (_, directory) = send(&app, "GET", "/-/pnpr/v0/registries", Some(&bob), None).await;
    assert_eq!(directory["admin"], json!(false));
}

#[tokio::test]
async fn team_edits_refuse_oversized_bodies() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let body = json!({ "name": "x".repeat(100 * 1024) });

    let (status, _) = send(&app, "PUT", "/npm/-/org/acme/team", None, Some(body)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn a_replica_that_cannot_read_the_graph_serves_no_registry() {
    let dir = TempDir::new().unwrap();
    let auth = pnpr::AuthState::in_memory();
    let (_, bob_id) = auth.users.add_or_login("bob", "secret123").await.unwrap();
    let yaml = CONFIG.replace("members: []", &format!("members: [{bob_id}]"));
    assert_ne!(yaml, CONFIG);
    let config = load_config_with(dir.path(), &yaml).await;
    let app = router_with_auth(config.clone(), auth.clone());
    let bob = add_user(&app, "bob").await;
    assert_eq!(
        send(&app, "GET", "/npm/@secret/thing", Some(&bob), None).await.0,
        StatusCode::NOT_FOUND,
    );
    let storage = pnpr_storage::Storage::from_config(&config.storage).unwrap();
    let current = storage.read_registry_graph_generation().await.unwrap();
    let unreadable = pnpr_storage::metadata::NewRegistryGraph {
        document: "{",
        actor_id: "-",
        restored_from: None,
        vacated: &[],
    };
    storage.write_registry_graph(current, unreadable).await.unwrap();
    let app = router_with_auth(config, auth);

    for path in ["/npm/@secret/thing", "/npm/lodash"] {
        let (status, error) = send(&app, "GET", path, Some(&bob), None).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{path}");
        assert_eq!(error["code"], "ERR_PNPR_REGISTRIES_UNAVAILABLE");
    }
}

#[tokio::test]
async fn deleting_a_member_prevents_reclaiming_its_team_access() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let former = add_user(&app, "former-member").await;
    let former_id = crate::tokens::account_id(&app, "former-member", Some(&root))
        .await
        .unwrap();
    let roster = "/npm/-/team/acme/readers/user";
    let member = json!({ "user": "former-member" });
    assert_eq!(send(&app, "PUT", roster, Some(&root), Some(member)).await.0, StatusCode::CREATED);
    assert_eq!(
        send(&app, "GET", "/npm/@secret/thing", Some(&former), None).await.0,
        StatusCode::NOT_FOUND,
    );
    assert_eq!(
        send(&app, "DELETE", &format!("/-/pnpr/v0/admin/users/{former_id}"), Some(&root), None)
            .await
            .0,
        StatusCode::NO_CONTENT,
    );
    let (_, members) = send(&app, "GET", roster, Some(&root), None).await;
    assert_eq!(members, json!([{ "id": former_id, "name": format!("deleted:{former_id}") }]));
    let registration = json!({ "name": "former-member", "password": "secret123" });
    assert_eq!(
        send(&app, "PUT", "/-/user/org.couchdb.user:former-member", None, Some(registration))
            .await
            .0,
        StatusCode::CREATED,
    );
    let replacement = add_user(&app, "former-member").await;
    assert_eq!(
        send(&app, "GET", "/npm/@secret/thing", Some(&replacement), None).await.0,
        StatusCode::FORBIDDEN,
    );
    assert_eq!(
        send(&app, "GET", "/npm/@secret/thing", Some(&former), None).await.0,
        StatusCode::UNAUTHORIZED,
    );
}

#[tokio::test]
async fn deleted_members_can_be_removed_by_id_after_their_name_is_reused() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    add_user(&app, "member").await;
    let former_id = crate::tokens::account_id(&app, "member", Some(&root)).await.unwrap();
    let roster = "/npm/-/team/acme/readers/user";
    let member = json!({"user": "member"});
    assert_eq!(
        send(&app, "PUT", roster, Some(&root), Some(member.clone())).await.0,
        StatusCode::CREATED,
    );
    assert_eq!(
        send(&app, "DELETE", &format!("/-/pnpr/v0/admin/users/{former_id}"), Some(&root), None)
            .await
            .0,
        StatusCode::NO_CONTENT,
    );
    let replacement = add_user(&app, "member").await;
    let replacement_id = crate::tokens::account_id(&app, "member", Some(&root))
        .await
        .unwrap();
    assert_ne!(former_id, replacement_id);
    assert_eq!(send(&app, "PUT", roster, Some(&root), Some(member)).await.0, StatusCode::CREATED);
    let ambiguous = json!({"userId": former_id, "user": "member"});
    assert_eq!(
        send(&app, "DELETE", roster, Some(&root), Some(ambiguous)).await.0,
        StatusCode::BAD_REQUEST,
    );
    let removal = json!({"userId": former_id});
    assert_eq!(
        send(&app, "DELETE", roster, Some(&root), Some(removal.clone())).await.0,
        StatusCode::NO_CONTENT,
    );
    let (_, members) = send(&app, "GET", roster, Some(&root), None).await;
    assert_eq!(members, json!([{"id": replacement_id, "name": "member"}]));
    assert_eq!(
        send(&app, "GET", "/npm/@secret/thing", Some(&replacement), None).await.0,
        StatusCode::NOT_FOUND,
    );
    assert_eq!(
        send(&app, "DELETE", roster, Some(&root), Some(removal)).await.0,
        StatusCode::NO_CONTENT,
    );
}

#[tokio::test]
async fn only_members_and_admins_can_read_teams() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let member = add_user(&app, "member").await;
    let outsider = add_user(&app, "outsider").await;
    let roster = "/npm/-/team/acme/readers/user";
    assert_eq!(
        send(&app, "PUT", roster, Some(&root), Some(json!({"user":"member"})))
            .await
            .0,
        StatusCode::CREATED,
    );
    for path in [
        "/npm/-/org/acme/team",
        "/npm/-/team/acme/readers/user",
        "/npm/-/team/acme/readers/package",
        "/npm/~local/-/org/acme/team",
        "/npm/~local/-/team/acme/readers/user",
        "/npm/~local/-/team/acme/readers/package",
    ] {
        for (token, expected) in [
            (Some(root.as_str()), StatusCode::OK),
            (Some(member.as_str()), StatusCode::OK),
            (Some(outsider.as_str()), StatusCode::NOT_FOUND),
            (None, StatusCode::UNAUTHORIZED),
        ] {
            assert_eq!(send(&app, "GET", path, token, None).await.0, expected, "{path}");
        }
    }
    assert_eq!(
        send(&app, "DELETE", roster, Some(&root), Some(json!({"user":"member"})))
            .await
            .0,
        StatusCode::NO_CONTENT,
    );
    assert_eq!(
        send(&app, "GET", "/npm/-/org/acme/team", Some(&member), None).await.0,
        StatusCode::NOT_FOUND,
    );
}
