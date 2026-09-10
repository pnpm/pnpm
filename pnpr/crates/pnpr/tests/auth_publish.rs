//! Integration tests for the auth, dist-tag, and publish endpoints.
//! Static-mode (no upstream) to keep the tests hermetic.

#[path = "auth_publish/scoped_publishing.rs"]
mod scoped_publishing;

#[path = "auth_publish/publish_validation.rs"]
mod publish_validation;

#[path = "auth_publish/behavior.rs"]
mod behavior;

#[path = "auth_publish/authentication.rs"]
mod authentication;

// `#[path]` rather than the `tests/common/mod.rs` layout, which the
// Perfectionist dylint forbids.
#[path = "common/storage.rs"]
mod common;
#[path = "common/npm.rs"]
mod npm;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use base64::engine::general_purpose::STANDARD as BASE64;
use npm::{publish_doc, sha1_hex, sri_sha512};
use pnpr::{Config, MaxUsers, router};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::PathBuf,
};
use tempfile::TempDir;
use tower::ServiceExt;

fn static_config(storage: PathBuf) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::static_serve(listen, storage);
    config.public_url = "http://example.test".to_string();
    config.auth.htpasswd.max_users = MaxUsers::Unlimited;
    config
}

fn static_config_with_packages(dir: &TempDir, packages_block: &str) -> (Config, PathBuf) {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let storage = dir.path().join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    // Route everything to one local hosted over the flat storage root (an
    // empty `org` namespace), so publishes/reads resolve through the
    // path-less base and the per-package rules in `packages_block` gate
    // them. The callers indent keys by two spaces; re-indent under
    // `registries.local.packages`, and add a `'**'` catch-all with the
    // default rules so the namespace stays name-unbounded like the old
    // static shape.
    let nested: String = packages_block
        .lines()
        .map(|line| if line.trim().is_empty() { line.to_string() } else { format!("    {line}") })
        .collect::<Vec<_>>()
        .join("\n");
    let yaml = format!(
        "storage: {}\nregistries:\n  \
         local:\n    type: hosted\n    org: \"\"\n    access: $all\n    packages:\n\
         {nested}\n      '**': {{}}\n  \
         main:\n    type: router\n    sources: [local]\n\
         defaultRegistry: main\n",
        storage.display(),
    );
    let config_path = dir.path().join("config.yaml");
    std::fs::write(&config_path, yaml).unwrap();
    let mut config =
        Config::from_yaml(&config_path, listen, Some("http://example.test".to_string())).unwrap();
    config.auth.htpasswd.max_users = MaxUsers::Unlimited;
    (config, storage)
}

async fn body_bytes(body: Body) -> Vec<u8> {
    to_bytes(body, usize::MAX).await.expect("read body").to_vec()
}

async fn body_json(body: Body) -> Value {
    serde_json::from_slice(&body_bytes(body).await).expect("body parses as JSON")
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called from multiple sites with owned literals; by-value keeps the call sites clean"
)]
fn put_json(path: &str, body: Value) -> Request<Body> {
    Request::put(path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

/// Drive an adduser PUT and pull the token out of the response. The
/// rest of the tests reuse this to get a bearer token.
async fn add_user_and_get_token(
    app: axum::Router,
    username: &str,
    password: &str,
) -> (axum::Router, String) {
    let path = format!("/-/user/org.couchdb.user:{username}");
    let body = json!({
        "_id": format!("org.couchdb.user:{username}"),
        "name": username,
        "password": password,
        "email": "foo@bar.net",
        "type": "user",
        "roles": [],
    });
    let response = app.clone().oneshot(put_json(&path, body)).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let payload = body_json(response.into_body()).await;
    let token = payload["token"].as_str().expect("token in response").to_string();
    (app, token)
}
