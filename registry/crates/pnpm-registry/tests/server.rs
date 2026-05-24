use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

use pnpm_registry::{Config, router};

fn config_for(upstream: &str, cache_dir: std::path::PathBuf) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::new(listen, cache_dir);
    config.upstream = upstream.to_string();
    config.public_url = "http://example.test".to_string();
    config.packument_ttl = Duration::from_secs(60);
    config
}

async fn body_bytes(body: Body) -> Vec<u8> {
    to_bytes(body, usize::MAX).await.expect("read body").to_vec()
}

#[tokio::test]
async fn packument_is_proxied_cached_and_rewritten() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({
        "name": "foo",
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()),
                    "shasum": "deadbeef"
                }
            }
        }
    });
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.public_url = "http://example.test".to_string();
    let app = router(config).unwrap();

    let response =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(
        body["versions"]["1.0.0"]["dist"]["tarball"],
        "http://example.test/foo/-/foo-1.0.0.tgz",
    );
    assert_eq!(body["versions"]["1.0.0"]["dist"]["shasum"], "deadbeef");

    let cached =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(cached.status(), StatusCode::OK);

    packument_mock.assert_async().await;
}

#[tokio::test]
async fn scoped_packument_is_served() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({ "name": "@types/node", "versions": {} });
    let mock = upstream
        .mock("GET", "/@types/node")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf())).unwrap();

    let response =
        app.oneshot(Request::get("/@types/node").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    mock.assert_async().await;
}

#[tokio::test]
async fn tarball_is_proxied_and_cached() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"fake-tarball-bytes";
    let mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_header("content-type", "application/octet-stream")
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf())).unwrap();

    let first = app
        .clone()
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(body_bytes(first.into_body()).await, bytes);

    let second = app
        .clone()
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(body_bytes(second.into_body()).await, bytes);

    mock.assert_async().await;
}

#[tokio::test]
async fn upstream_404_is_propagated() {
    let mut upstream = mockito::Server::new_async().await;
    let _mock = upstream
        .mock("GET", "/missing")
        .with_status(404)
        .with_body("Not Found")
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf())).unwrap();

    let response =
        app.oneshot(Request::get("/missing").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tarball_filename_for_other_package_is_rejected() {
    let upstream = mockito::Server::new_async().await;
    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf())).unwrap();

    let response = app
        .oneshot(Request::get("/foo/-/bar-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
