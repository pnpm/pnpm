//! npm's team package access endpoints (`npm access grant`, `revoke`, and
//! `list packages`) on registries the metadata database stores
//! (`registriesManagedBy: api`): what a grant
//! writes into the package rules, who may make one, and which packages it
//! applies to.

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
      devs: { name: devs, members: [carol] }
    packages:
      '@acme/app':
        access: [root]
        publish: [root]
      '@acme/*': {}
  fixed:
    type: hosted
    namespace: fixed
    teams:
      devs: { name: devs, members: [carol] }
    packages:
      '@fixed/*': {}
  main:
    type: router
    sources: [fixed, local]
defaultRegistry: main
";

const PACKAGES: &str = "/npm/-/team/acme/devs/package";
const RULES: &str = "/-/pnpr/v0/admin/rules/npm/local";

async fn load_config(dir: &Path) -> Config {
    let yaml = format!("storage: {}\n{CONFIG}", storage_dir(dir).display());
    crate::managed_registries::managed_config(
        &dir.join("config.yaml"),
        &yaml,
        listen(),
        public_url(),
    )
    .await
}

/// [`CONFIG`] with the registries in the config file.
fn load_file_config(dir: &Path) -> Config {
    let path = dir.join("config.yaml");
    std::fs::write(&path, format!("storage: {}\n{CONFIG}", storage_dir(dir).display())).unwrap();
    Config::from_yaml(&path, listen(), public_url()).unwrap()
}

fn storage_dir(dir: &Path) -> std::path::PathBuf {
    let storage = dir.join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    storage
}

fn listen() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873))
}

fn public_url() -> Option<String> {
    Some("http://example.test".to_string())
}

async fn app_router(config: Config) -> axum::Router {
    let auth =
        crate::tokens::seeded_auth(&config, &[("root", "secret123"), ("carol", "secret123")]);
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

/// The package does not exist, so 404 means the read was allowed and 403
/// that it was refused.
async fn read_app(app: &axum::Router, token: &str) -> StatusCode {
    send(app, "GET", "/npm/@acme/app", Some(token), None).await.0
}

async fn app_rule(app: &axum::Router, root: &str) -> Value {
    let (status, rules) = send(app, "GET", RULES, Some(root), None).await;
    assert_eq!(status, StatusCode::OK);
    rules["packages"]["@acme/app"].clone()
}

#[tokio::test]
async fn a_grant_edits_the_package_rule_and_a_revoke_undoes_it() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let carol = add_user(&app, "carol").await;
    assert_eq!(read_app(&app, &carol).await, StatusCode::FORBIDDEN);

    let grant = json!({ "package": "@acme/app", "permissions": "read-write" });
    assert_eq!(send(&app, "PUT", PACKAGES, Some(&root), Some(grant)).await.0, StatusCode::CREATED);
    assert_eq!(read_app(&app, &carol).await, StatusCode::NOT_FOUND);
    let rule = app_rule(&app, &root).await;
    assert_eq!(rule["access"], json!(["root", "team:devs"]));
    assert_eq!(rule["publish"], json!(["root", "team:devs"]));
    let (status, listed) = send(&app, "GET", PACKAGES, Some(&carol), None).await;
    assert_eq!((status, listed), (StatusCode::OK, json!({ "@acme/app": "read-write" })));

    let grant = json!({ "package": "@acme/app", "permissions": "read-only" });
    assert_eq!(send(&app, "PUT", PACKAGES, Some(&root), Some(grant)).await.0, StatusCode::CREATED);
    assert_eq!(app_rule(&app, &root).await["publish"], json!(["root"]));
    let (_, listed) = send(&app, "GET", PACKAGES, Some(&carol), None).await;
    assert_eq!(listed, json!({ "@acme/app": "read-only" }));

    let revoke = json!({ "package": "@acme/app" });
    assert_eq!(
        send(&app, "DELETE", PACKAGES, Some(&root), Some(revoke)).await.0,
        StatusCode::NO_CONTENT,
    );
    assert_eq!(read_app(&app, &carol).await, StatusCode::FORBIDDEN);
    assert_eq!(app_rule(&app, &root).await["access"], json!(["root"]));
    let (_, listed) = send(&app, "GET", PACKAGES, Some(&root), None).await;
    assert_eq!(listed, json!({}));
}

#[tokio::test]
async fn a_grant_stores_only_the_lists_it_changes() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let grant = json!({ "package": "@acme/app", "permissions": "read-only" });
    assert_eq!(send(&app, "PUT", PACKAGES, Some(&root), Some(grant)).await.0, StatusCode::CREATED);

    let (status, document) =
        send(&app, "GET", "/-/pnpr/v0/admin/registries", Some(&root), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        document["registries"]["local"]["packages"]["@acme/app"],
        json!({ "access": ["root", "team:devs"], "publish": ["root"] }),
    );
}

#[tokio::test]
async fn grants_need_an_admin_a_declared_package_and_a_known_team() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let carol = add_user(&app, "carol").await;
    let grant = |package: &str| json!({ "package": package, "permissions": "read-write" });

    let cases = [
        (PACKAGES, &carol, grant("@acme/app"), StatusCode::FORBIDDEN),
        (PACKAGES, &root, grant("@acme/other"), StatusCode::BAD_REQUEST),
        (
            PACKAGES,
            &root,
            json!({ "package": "@acme/app", "permissions": "admin" }),
            StatusCode::BAD_REQUEST,
        ),
        ("/npm/-/team/acme/nobody/package", &root, grant("@acme/app"), StatusCode::NOT_FOUND),
        ("/npm/-/team/fixed/devs/package", &root, grant("@fixed/app"), StatusCode::BAD_REQUEST),
    ];
    for (path, token, body, expected) in cases {
        let (status, _) = send(&app, "PUT", path, Some(token), Some(body.clone())).await;
        assert_eq!(status, expected, "{path} {body}");
    }
    assert_eq!(app_rule(&app, &root).await["access"], json!(["root"]));
}

#[tokio::test]
async fn grants_need_registries_the_database_stores() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_file_config(dir.path())).await;
    let root = add_user(&app, "root").await;
    let grant = json!({ "package": "@acme/app", "permissions": "read-write" });

    let (status, error) = send(&app, "PUT", PACKAGES, Some(&root), Some(grant)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["code"], "ERR_PNPR_RULES_CONFIG_MANAGED");
}
