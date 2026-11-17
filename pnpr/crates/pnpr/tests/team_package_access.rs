//! npm's team package access endpoints (`npm access grant`, `revoke`, and
//! `list packages`): what a grant writes into the stored package rules, who
//! may make one, and which packages it applies to.

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
    packages:
      '@acme/app':
        access: [root]
        publish: [root]
      '@acme/lib': {}
      '@acme/*': {}
  fixed:
    type: hosted
    namespace: fixed
    packages:
      '@fixed/*': {}
  main:
    type: router
    sources: [fixed, local]
defaultRegistry: main
teams:
  devs: { name: devs, members: [carol] }
";

const PACKAGES: &str = "/npm/-/team/acme/devs/package";

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

async fn team_packages(app: &axum::Router, token: &str) -> Value {
    let (status, packages) = send(app, "GET", PACKAGES, Some(token), None).await;
    assert_eq!(status, StatusCode::OK, "{packages}");
    packages
}

#[tokio::test]
async fn a_grant_gives_the_team_the_package_and_a_revoke_takes_it_back() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let carol = add_user(&app, "carol").await;
    assert_eq!(read_app(&app, &carol).await, StatusCode::FORBIDDEN);
    assert_eq!(team_packages(&app, &root).await, json!({}));

    let grant = json!({ "package": "@acme/app", "permissions": "read-only" });
    assert_eq!(send(&app, "PUT", PACKAGES, Some(&root), Some(grant)).await.0, StatusCode::CREATED);
    assert_eq!(read_app(&app, &carol).await, StatusCode::NOT_FOUND);
    assert_eq!(team_packages(&app, &carol).await, json!({ "@acme/app": "read-only" }));

    // Granting again replaces what the team has on the package.
    let grant = json!({ "package": "@acme/app", "permissions": "read-write" });
    assert_eq!(send(&app, "PUT", PACKAGES, Some(&root), Some(grant)).await.0, StatusCode::CREATED);
    assert_eq!(team_packages(&app, &root).await, json!({ "@acme/app": "read-write" }));

    let revoke = json!({ "package": "@acme/app" });
    for _ in 0..2 {
        let (status, _) = send(&app, "DELETE", PACKAGES, Some(&root), Some(revoke.clone())).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    assert_eq!(read_app(&app, &carol).await, StatusCode::FORBIDDEN);
    assert_eq!(team_packages(&app, &root).await, json!({}));
}

#[tokio::test]
async fn a_grant_is_one_grant_of_the_instance() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let grant = json!({ "package": "@acme/lib", "permissions": "read-write" });
    assert_eq!(send(&app, "PUT", PACKAGES, Some(&root), Some(grant)).await.0, StatusCode::CREATED);

    let (_, listed) = send(&app, "GET", "/-/pnpr/v0/admin/grants", Some(&root), None).await;
    let granted: Vec<&Value> = listed["grants"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|grant| grant["include"] == json!(["@acme/lib"]) && grant["exclude"] == json!([]))
        .filter(|grant| grant["subjects"].get("team:devs").is_some())
        .collect();
    assert_eq!(granted.len(), 1, "{listed}");
    assert_eq!(granted[0]["registries"], json!(["local"]));
    assert_eq!(
        granted[0]["subjects"]["team:devs"],
        json!(["annotate", "cache", "publish", "read"]),
    );
}

#[tokio::test]
async fn the_listing_adds_up_grants_and_leaves_out_what_a_grant_excludes() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let grants = [
        ("readers", json!({ "include": ["@acme/app"], "subjects": { "team:devs": ["read"] } })),
        ("writers", json!({ "include": ["@acme/app"], "subjects": { "team:devs": ["publish"] } })),
        (
            "nothing",
            json!({
                "include": ["@acme/lib"],
                "exclude": ["@acme/lib"],
                "subjects": { "team:devs": ["read", "publish"] },
            }),
        ),
    ];
    for (name, mut grant) in grants {
        grant["registries"] = json!(["local"]);
        let path = format!("/-/pnpr/v0/admin/grants/{name}");
        let (status, body) = send(&app, "PUT", &path, Some(&root), Some(grant)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    assert_eq!(team_packages(&app, &root).await, json!({ "@acme/app": "read-write" }));
}

#[tokio::test]
async fn grants_need_an_admin_and_a_known_team() {
    let dir = TempDir::new().unwrap();
    let app = app_router(load_config(dir.path()).await).await;
    let root = add_user(&app, "root").await;
    let carol = add_user(&app, "carol").await;
    let grant = json!({ "package": "@acme/app", "permissions": "read-write" });
    assert_eq!(
        send(&app, "PUT", PACKAGES, Some(&carol), Some(grant.clone())).await.0,
        StatusCode::FORBIDDEN,
    );
    assert_eq!(
        send(&app, "PUT", "/npm/-/team/acme/absent/package", Some(&root), Some(grant))
            .await
            .0,
        StatusCode::NOT_FOUND,
    );
    let malformed = json!({ "package": "@acme/**/x", "permissions": "read-only" });
    assert_eq!(
        send(&app, "PUT", PACKAGES, Some(&root), Some(malformed)).await.0,
        StatusCode::BAD_REQUEST,
    );
}
