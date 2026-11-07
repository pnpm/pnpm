//! Team administration through the npm team API on a registry whose teams
//! the API manages (`teamsManagedBy: api`): who may edit the roster, how an
//! edit reaches the `team:` rules, and that the roster lives in the hosted
//! store rather than in the process.

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
    org: \"\"
    teamsManagedBy: api
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

fn load_config(dir: &Path) -> Config {
    load_config_with(dir, CONFIG)
}

fn load_config_with(dir: &Path, yaml: &str) -> Config {
    let storage = dir.join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    let path = dir.join("config.yaml");
    std::fs::write(&path, format!("storage: {}\n{yaml}", storage.display())).unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    Config::from_yaml(&path, listen, Some("http://example.test".to_string())).unwrap()
}

async fn app_router(config: Config) -> axum::Router {
    let auth = crate::tokens::seeded_auth(&config, &[("root", "secret")]);
    router_with_auth(config, auth)
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
    let app = app_router(load_config(dir.path())).await;
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
    assert_eq!(members[0]["id"].as_str(), crate::tokens::account_id(&app, "bob").await.as_deref());

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
    let app = app_router(load_config(dir.path())).await;
    let bob = add_user(&app, "bob").await;
    let create = json!({ "name": "ops" });

    let (status, _) =
        send(&app, "PUT", "/npm/-/org/acme/team", Some(&bob), Some(create.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(&app, "PUT", "/npm/-/org/acme/team", None, Some(create)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(team_names(&app, &bob).await, json!([{ "name": "readers" }]));
}

#[tokio::test]
async fn roster_edits_refuse_conflicting_state() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path())).await;
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
    let app = app_router(load_config(dir.path())).await;
    let root = add_user(&app, "root").await;
    let create = json!({ "name": "ops" });
    assert_eq!(
        send(&app, "PUT", "/npm/-/org/acme/team", Some(&root), Some(create))
            .await
            .0,
        StatusCode::CREATED,
    );

    let restarted = app_router(load_config(dir.path())).await;
    let bob = add_user(&restarted, "bob").await;
    assert_eq!(
        team_names(&restarted, &bob).await,
        json!([{ "name": "readers" }, { "name": "ops" }]),
    );
}

#[tokio::test]
async fn directory_reports_admin_and_team_management() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path())).await;
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
    let app = app_router(load_config(dir.path())).await;
    let body = json!({ "name": "x".repeat(100 * 1024) });

    let (status, _) = send(&app, "PUT", "/npm/-/org/acme/team", None, Some(body)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn an_unreadable_roster_denies_team_grants_and_nothing_else() {
    let dir = TempDir::new().unwrap();
    let auth = pnpr::AuthState::in_memory();
    let (_, bob_id) = auth.users.add_or_login("bob", "secret").await.unwrap();
    let yaml = CONFIG.replace("members: []", &format!("members: [{bob_id}]"));
    assert_ne!(yaml, CONFIG);
    let config = load_config_with(dir.path(), &yaml);
    let app = router_with_auth(config.clone(), auth.clone());
    let bob = add_user(&app, "bob").await;
    assert_eq!(
        send(&app, "GET", "/npm/@secret/thing", Some(&bob), None).await.0,
        StatusCode::NOT_FOUND,
    );
    let roster = dir.path().join("storage/.team-rosters/npm/local.json");
    std::fs::create_dir_all(roster.parent().unwrap()).unwrap();
    std::fs::write(&roster, "not a roster").unwrap();
    let app = router_with_auth(config, auth);

    let (status, _) = send(&app, "GET", "/npm/@secret/thing", Some(&bob), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(&app, "GET", "/npm/lodash", Some(&bob), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_a_member_prevents_reclaiming_its_team_access() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path())).await;
    let root = add_user(&app, "root").await;
    let former = add_user(&app, "former-member").await;
    let former_id = crate::tokens::account_id(&app, "former-member").await.unwrap();
    let roster = "/npm/-/team/acme/readers/user";
    let member = json!({ "user": "former-member" });
    assert_eq!(send(&app, "PUT", roster, Some(&root), Some(member)).await.0, StatusCode::CREATED);
    assert_eq!(
        send(&app, "GET", "/npm/@secret/thing", Some(&former), None).await.0,
        StatusCode::NOT_FOUND,
    );
    assert_eq!(
        send(&app, "DELETE", "/-/pnpr/v0/admin/users/former-member", Some(&root), None)
            .await
            .0,
        StatusCode::NO_CONTENT,
    );
    let (_, members) = send(&app, "GET", roster, Some(&root), None).await;
    assert_eq!(members, json!([{ "id": former_id, "name": format!("deleted:{former_id}") }]));
    let registration = json!({ "name": "former-member", "password": "secret" });
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
    let app = app_router(load_config(dir.path())).await;
    let root = add_user(&app, "root").await;
    add_user(&app, "member").await;
    let former_id = crate::tokens::account_id(&app, "member").await.unwrap();
    let roster = "/npm/-/team/acme/readers/user";
    let member = json!({"user": "member"});
    assert_eq!(
        send(&app, "PUT", roster, Some(&root), Some(member.clone())).await.0,
        StatusCode::CREATED,
    );
    assert_eq!(
        send(&app, "DELETE", "/-/pnpr/v0/admin/users/member", Some(&root), None)
            .await
            .0,
        StatusCode::NO_CONTENT,
    );
    let replacement = add_user(&app, "member").await;
    let replacement_id = crate::tokens::account_id(&app, "member").await.unwrap();
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
