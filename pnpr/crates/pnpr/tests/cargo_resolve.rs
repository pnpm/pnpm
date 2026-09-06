//! Cargo resolution through the install accelerator: `POST
//! /-/pnpr/v0/resolve` with `"ecosystem": "cargo"`.
//!
//! The client sends its `cargo metadata` output and the sparse index it
//! resolves against; the server walks that index and answers with a
//! rendered `Cargo.lock`. These tests stand a mock sparse index up and
//! assert what the server fetched from it as much as what it returned.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::{AuthState, Config, PublicRoute, router_with_auth};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::PathBuf,
    time::Duration,
};
use tempfile::TempDir;
use tower::ServiceExt;

/// A checksum shaped the way `Cargo.lock` requires: 64 hex digits.
const CKSUM: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn config_for(storage: PathBuf) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::proxy(listen, storage);
    config.public_url = "http://example.test".to_string();
    config.packument_ttl = Duration::from_mins(1);
    config
}

/// `cargo metadata --no-deps` for a one-package workspace depending on
/// `foo`, whose own `bar` dependency is only discoverable from the index.
fn workspace_metadata() -> String {
    json!({
        "packages": [{
            "id": "app 0.1.0",
            "name": "app",
            "version": "0.1.0",
            "features": {},
            "dependencies": [{
                "name": "foo",
                "source": "registry+https://github.com/rust-lang/crates.io-index",
                "req": "^1",
                "kind": null,
                "optional": false,
                "uses_default_features": true,
                "features": [],
            }],
        }],
        "workspace_members": ["app 0.1.0"],
    })
    .to_string()
}

fn index_entry(name: &str, version: &str, dependencies: &Value) -> String {
    json!({
        "name": name,
        "vers": version,
        "deps": dependencies,
        "cksum": CKSUM,
        "features": {},
        "yanked": false,
    })
    .to_string()
}

fn index_dependency(name: &str, requirement: &str) -> Value {
    json!({
        "name": name,
        "req": requirement,
        "features": [],
        "optional": false,
        "default_features": true,
        "target": null,
        "kind": "normal",
    })
}

/// A sparse index serving `foo@1.0.0` (which depends on `bar`) and
/// `bar@1.0.0`, with each entry's mock asserting how often it was fetched.
async fn sparse_index(hits_per_entry: usize) -> (mockito::ServerGuard, Vec<mockito::Mock>) {
    let mut index = mockito::Server::new_async().await;
    let entries = [
        ("/3/f/foo", index_entry("foo", "1.0.0", &json!([index_dependency("bar", "^1")]))),
        ("/3/b/bar", index_entry("bar", "1.0.0", &json!([]))),
    ];
    let mut mocks = Vec::new();
    for (path, body) in entries {
        mocks.push(
            index
                .mock("GET", path)
                .with_status(200)
                .with_body(body)
                .expect(hits_per_entry)
                .create_async()
                .await,
        );
    }
    (index, mocks)
}

fn cargo_resolve_request(registry: &str, token: &str) -> Request<Body> {
    let body = json!({
        "ecosystem": "cargo",
        "metadata": workspace_metadata(),
        "registry": registry,
    });
    Request::post("/-/pnpr/v0/resolve")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

async fn frames(body: Body) -> Vec<Value> {
    let bytes = to_bytes(body, usize::MAX).await.expect("read body");
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).expect("frame is JSON"))
        .collect()
}

/// The `Cargo.lock` a successful resolve returned.
async fn resolved_lockfile(response: axum::response::Response) -> String {
    assert_eq!(response.status(), StatusCode::OK);
    let frames = frames(response.into_body()).await;
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!(frames[0]["type"], "done", "{frames:?}");
    frames[0]["lockfile"].as_str().expect("done frame carries the lockfile").to_string()
}

#[tokio::test]
async fn cargo_resolve_walks_the_index_and_returns_a_lockfile() {
    let (index, mocks) = sparse_index(1).await;
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let mut config = config_for(tmp.path().to_path_buf());
    config.route_policy.public.push(PublicRoute { registry: Some(index.url()), package: None });
    let app = router_with_auth(config, auth);

    let response = app.oneshot(cargo_resolve_request(&index.url(), &token)).await.unwrap();
    let lockfile = resolved_lockfile(response).await;

    // `bar` is only reachable through `foo`'s index entry, so its presence
    // proves the server kept walking the index until nothing was missing.
    assert!(lockfile.contains(r#"name = "foo""#), "{lockfile}");
    assert!(lockfile.contains(r#"name = "bar""#), "{lockfile}");
    assert!(lockfile.contains(r#"name = "app""#), "{lockfile}");
    for mock in mocks {
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn cargo_resolve_reuses_cached_index_files() {
    let (index, mocks) = sparse_index(1).await;
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let mut config = config_for(tmp.path().to_path_buf());
    config.route_policy.public.push(PublicRoute { registry: Some(index.url()), package: None });
    let app = router_with_auth(config, auth);

    let first = app.clone().oneshot(cargo_resolve_request(&index.url(), &token)).await.unwrap();
    let first = resolved_lockfile(first).await;
    let second = app.oneshot(cargo_resolve_request(&index.url(), &token)).await.unwrap();
    let second = resolved_lockfile(second).await;

    assert_eq!(first, second);
    // Each entry is mocked with `expect(1)`: the second resolve read the
    // cached index files rather than refetching them.
    for mock in mocks {
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn cargo_resolve_rejects_an_off_allowlist_registry() {
    let (index, mocks) = sparse_index(0).await;
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    // No public route for the index: the operator never declared it.
    let app = router_with_auth(config_for(tmp.path().to_path_buf()), auth);

    let response = app.oneshot(cargo_resolve_request(&index.url(), &token)).await.unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    for mock in mocks {
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn cargo_resolve_rejects_a_registry_with_inline_credentials() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config_for(tmp.path().to_path_buf()), auth);

    let response = app
        .oneshot(cargo_resolve_request("https://user:secret@index.example.test", &token))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn cargo_resolve_reports_an_unresolvable_workspace() {
    let mut index = mockito::Server::new_async().await;
    let missing = index.mock("GET", "/3/f/foo").with_status(404).create_async().await;
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let mut config = config_for(tmp.path().to_path_buf());
    config.route_policy.public.push(PublicRoute { registry: Some(index.url()), package: None });
    let app = router_with_auth(config, auth);

    let response = app.oneshot(cargo_resolve_request(&index.url(), &token)).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let frames = frames(response.into_body()).await;
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!(frames[0]["type"], "error", "{frames:?}");
    assert!(
        frames[0]["message"].as_str().expect("error frame carries a message").contains("404"),
        "{frames:?}",
    );
    missing.assert_async().await;
}

#[tokio::test]
async fn resolve_rejects_an_ecosystem_it_does_not_serve() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config_for(tmp.path().to_path_buf()), auth);

    let request = Request::post("/-/pnpr/v0/resolve")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(json!({ "ecosystem": "pypi" }).to_string()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.expect("read body");
    assert!(String::from_utf8_lossy(&bytes).contains("pypi"));
}

#[tokio::test]
async fn anonymous_cargo_resolve_is_rejected() {
    let (index, mocks) = sparse_index(0).await;
    let tmp = TempDir::new().unwrap();
    let mut config = config_for(tmp.path().to_path_buf());
    config.route_policy.public.push(PublicRoute { registry: Some(index.url()), package: None });
    let app = router_with_auth(config, AuthState::in_memory());

    let body = json!({
        "ecosystem": "cargo",
        "metadata": workspace_metadata(),
        "registry": index.url(),
    });
    let request = Request::post("/-/pnpr/v0/resolve")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    for mock in mocks {
        mock.assert_async().await;
    }
}
