//! The admin API for grants (`/-/pnpr/v0/admin/grants`): what a grant does to
//! requests, who may write one, and that grants live in the database.

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

/// `local` lets everyone read all but `@secret/*`, which only `root` reads.
/// `hidden` lets only `root` read anything.
const CONFIG: &str = "\
auth:
  admins: [root]
  registryAdmins: [root]
  users:
    maxUsers: 100
registries:
  local:
    type: hosted
    namespace: \"\"
    packages:
      '@secret/*':
        access: [root]
      '**': {}
  hidden:
    type: hosted
    namespace: hidden
    access: [root]
    packages:
      '@hidden/*': {}
  main:
    type: router
    sources: [hidden, local]
defaultRegistry: main
teams:
  readers: { name: readers, members: [carol] }
";

const GRANTS: &str = "/-/pnpr/v0/admin/grants";

async fn load_config(dir: &Path) -> Config {
    let storage = dir.join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    let yaml = format!("storage: {}\n{CONFIG}", storage.display());
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
    let auth = crate::tokens::seeded_auth(
        &config,
        &[("root", "secret123"), ("carol", "secret123"), ("bob", "secret123")],
    );
    router_with_auth(config, auth)
}

/// A request and the version `If-Match` names, when it names one.
struct Sent<'a> {
    method: &'a str,
    path: &'a str,
    token: Option<&'a str>,
    body: Option<Value>,
    if_match: Option<&'a str>,
}

async fn send_conditional(app: &axum::Router, sent: Sent<'_>) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(sent.method)
        .uri(sent.path)
        .header("content-type", "application/json");
    if let Some(token) = sent.token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if let Some(version) = sent.if_match {
        request = request.header("if-match", format!("\"{version}\""));
    }
    let body = sent.body.map(|body| Body::from(body.to_string())).unwrap_or_else(Body::empty);
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn send(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    send_conditional(app, Sent { method, path, token, body, if_match: None }).await
}

async fn login(app: &axum::Router, username: &str) -> String {
    let body = json!({ "name": username, "password": "secret123", "type": "user", "roles": [] });
    let path = format!("/-/user/org.couchdb.user:{username}");
    let (status, payload) = send(app, "PUT", &path, None, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED);
    payload["token"]
        .as_str()
        .expect("token in response")
        .to_string()
}

/// The status of a read of a package that does not exist: `404` when the
/// read was allowed or masked, `403` when it was refused out loud.
async fn read(app: &axum::Router, path: &str, token: &str) -> StatusCode {
    send(app, "GET", path, Some(token), None).await.0
}

async fn put_grant(
    app: &axum::Router,
    token: &str,
    name: &str,
    grant: Value,
) -> (StatusCode, Value) {
    send(app, "PUT", &format!("{GRANTS}/{name}"), Some(token), Some(grant)).await
}

async fn grant_names(app: &axum::Router, token: &str) -> Vec<String> {
    let (status, listed) = send(app, "GET", GRANTS, Some(token), None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    listed["grants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|grant| {
            grant["name"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

fn secret_readers(subjects: &Value) -> Value {
    json!({ "registries": ["local"], "include": ["@secret/*"], "subjects": subjects })
}

#[tokio::test]
async fn a_grant_applies_at_once_and_its_removal_undoes_it() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = login(&app, "root").await;
    let bob = login(&app, "bob").await;
    assert_eq!(read(&app, "/npm/@secret/thing", &bob).await, StatusCode::FORBIDDEN);

    let grant = secret_readers(&json!({ "bob": ["read"] }));
    let (status, stored) = put_grant(&app, &root, "secret-readers", grant).await;
    assert_eq!(status, StatusCode::OK, "{stored}");
    assert_eq!(stored["subjects"], json!({ "bob": ["read"] }));
    assert_eq!(read(&app, "/npm/@secret/thing", &bob).await, StatusCode::NOT_FOUND);
    assert!(grant_names(&app, &root).await.contains(&"secret-readers".to_string()));

    let path = format!("{GRANTS}/secret-readers");
    assert_eq!(send(&app, "DELETE", &path, Some(&root), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(read(&app, "/npm/@secret/thing", &bob).await, StatusCode::FORBIDDEN);
    assert_eq!(send(&app, "DELETE", &path, Some(&root), None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_team_grant_admits_the_members_of_the_team() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = login(&app, "root").await;
    let carol = login(&app, "carol").await;
    let bob = login(&app, "bob").await;
    let grant = secret_readers(&json!({ "team:readers": ["read"] }));
    assert_eq!(put_grant(&app, &root, "secret-readers", grant).await.0, StatusCode::OK);
    assert_eq!(read(&app, "/npm/@secret/thing", &carol).await, StatusCode::NOT_FOUND);
    assert_eq!(read(&app, "/npm/@secret/thing", &bob).await, StatusCode::FORBIDDEN);
    // The team cannot go while the grant names it.
    let (status, _) = send(&app, "DELETE", "/npm/-/team/acme/readers", Some(&root), None).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_denied_read_is_masked_only_from_a_caller_who_reads_nothing_there() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = login(&app, "root").await;
    let bob = login(&app, "bob").await;
    let hidden = "/npm/~hidden/@hidden/thing";
    assert_eq!(read(&app, hidden, &bob).await, StatusCode::NOT_FOUND);

    let grant = json!({
        "registries": ["hidden"],
        "include": ["@hidden/other"],
        "subjects": { "bob": ["read"] },
    });
    assert_eq!(put_grant(&app, &root, "hidden-other", grant).await.0, StatusCode::OK);
    assert_eq!(read(&app, "/npm/~hidden/@hidden/other", &bob).await, StatusCode::NOT_FOUND);
    assert_eq!(read(&app, hidden, &bob).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_grant_that_cannot_apply_is_refused() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = login(&app, "root").await;
    let refused = [
        json!({ "registries": [], "subjects": { "bob": ["read"] } }),
        json!({ "registries": ["absent"], "subjects": { "bob": ["read"] } }),
        json!({ "registries": ["main"], "subjects": { "bob": ["read"] } }),
        json!({ "registries": ["local"], "subjects": { "bob": ["own"] } }),
        json!({ "registries": ["local"], "subjects": { "$everyone": ["read"] } }),
        json!({ "registries": ["local"], "subjects": { "$owners": ["read"] } }),
        json!({ "registries": ["local"], "subjects": { "team:absent": ["read"] } }),
        json!({ "registries": ["local"], "include": ["@bad/**/x"], "subjects": {} }),
        json!({ "registries": ["*"], "include": ["@acme/*"], "subjects": {} }),
        json!({ "registries": ["local"], "unknown": true }),
    ];
    for grant in refused {
        let (status, error) = put_grant(&app, &root, "refused", grant.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{grant}: {error}");
    }
    assert!(!grant_names(&app, &root).await.contains(&"refused".to_string()));
}

#[tokio::test]
async fn a_change_based_on_a_stale_version_is_refused() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = login(&app, "root").await;
    let (_, listed) = send(&app, "GET", GRANTS, Some(&root), None).await;
    let version = listed["version"]
        .as_str()
        .unwrap()
        .to_string();
    let path = format!("{GRANTS}/secret-readers");
    let put = |subjects: Value, if_match: &'static str| Sent {
        method: "PUT",
        path: "/-/pnpr/v0/admin/grants/secret-readers",
        token: None,
        body: Some(secret_readers(&subjects)),
        if_match: Some(if_match),
    };
    let first = Sent { token: Some(&root), if_match: Some(&version), ..put(json!({}), "") };
    assert_eq!(send_conditional(&app, first).await.0, StatusCode::OK);
    let stale = Sent {
        token: Some(&root),
        if_match: Some(&version),
        ..put(json!({ "bob": ["read"] }), "")
    };
    assert_eq!(send_conditional(&app, stale).await.0, StatusCode::PRECONDITION_FAILED);
    let removal = Sent {
        method: "DELETE",
        path: &path,
        token: Some(&root),
        body: None,
        if_match: Some(&version),
    };
    assert_eq!(send_conditional(&app, removal).await.0, StatusCode::PRECONDITION_FAILED);
    let (_, stored) = send(&app, "GET", &path, Some(&root), None).await;
    assert_eq!(stored["subjects"], json!({}));
}

#[tokio::test]
async fn a_manager_changes_the_subjects_of_its_grant_and_nothing_else() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = login(&app, "root").await;
    let carol = login(&app, "carol").await;
    let bob = login(&app, "bob").await;
    let managed = json!({ "carol": ["manage", "read"] });
    assert_eq!(
        put_grant(&app, &root, "secret-readers", secret_readers(&managed)).await.0,
        StatusCode::OK,
    );
    let path = format!("{GRANTS}/secret-readers");

    // A caller the grant does not let manage it learns nothing of it.
    assert_eq!(send(&app, "GET", &path, Some(&bob), None).await.0, StatusCode::NOT_FOUND);
    let attempt = secret_readers(&json!({ "bob": ["read"] }));
    assert_eq!(put_grant(&app, &bob, "secret-readers", attempt).await.0, StatusCode::NOT_FOUND);
    assert_eq!(send(&app, "GET", GRANTS, Some(&carol), None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(send(&app, "DELETE", &path, Some(&carol), None).await.0, StatusCode::FORBIDDEN);

    assert_eq!(send(&app, "GET", &path, Some(&carol), None).await.0, StatusCode::OK);
    let widened = json!({ "carol": ["manage", "read"], "bob": ["read"] });
    assert_eq!(
        put_grant(&app, &carol, "secret-readers", secret_readers(&widened))
            .await
            .0,
        StatusCode::OK,
    );
    assert_eq!(read(&app, "/npm/@secret/thing", &bob).await, StatusCode::NOT_FOUND);

    let more_managers = json!({ "carol": ["manage", "read"], "bob": ["manage"] });
    let other_packages = json!({ "registries": ["local"], "subjects": widened });
    let other_registry =
        json!({ "registries": ["hidden"], "include": ["@secret/*"], "subjects": widened });
    for refused in [secret_readers(&more_managers), other_packages, other_registry] {
        let (status, error) = put_grant(&app, &carol, "secret-readers", refused.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{refused}: {error}");
    }
    assert_eq!(
        put_grant(&app, &carol, "another", secret_readers(&widened)).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn adding_a_registry_seeds_its_grant_and_removing_it_forgets_the_grant() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = login(&app, "root").await;
    let bob = login(&app, "bob").await;
    let registry = "/-/pnpr/v0/admin/registries/extra";
    let entry = json!({ "type": "hosted", "namespace": "extra" });
    assert_eq!(send(&app, "PUT", registry, Some(&root), Some(entry)).await.0, StatusCode::OK);

    let (status, seeded) = send(&app, "GET", &format!("{GRANTS}/extra"), Some(&root), None).await;
    assert_eq!(status, StatusCode::OK, "{seeded}");
    assert_eq!(seeded["registries"], json!(["extra"]));
    assert_eq!(seeded["subjects"], json!({ "$all": ["read"], "$owners": ["annotate", "publish"] }));
    assert_eq!(read(&app, "/npm/~extra/thing", &bob).await, StatusCode::NOT_FOUND);

    let shared = json!({ "registries": ["extra", "local"], "subjects": { "bob": ["publish"] } });
    assert_eq!(put_grant(&app, &root, "shared", shared).await.0, StatusCode::OK);
    assert_eq!(send(&app, "DELETE", registry, Some(&root), None).await.0, StatusCode::OK);
    assert!(!grant_names(&app, &root).await.contains(&"extra".to_string()));
    let (_, shared) = send(&app, "GET", &format!("{GRANTS}/shared"), Some(&root), None).await;
    assert_eq!(shared["registries"], json!(["local"]));
}

#[tokio::test]
async fn a_new_process_serves_the_stored_grants() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = login(&app, "root").await;
    let grant = secret_readers(&json!({ "bob": ["read"] }));
    assert_eq!(put_grant(&app, &root, "secret-readers", grant).await.0, StatusCode::OK);

    let restarted = app_router(reload_config(dir.path())).await;
    let bob = login(&restarted, "bob").await;
    assert_eq!(read(&restarted, "/npm/@secret/thing", &bob).await, StatusCode::NOT_FOUND);
}
