mod cargo;

use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use pnpr_config::Config;
use tempfile::TempDir;
use tower::ServiceExt as _;

use super::{app_with_config_and_token, record};

const ENTRY: &str = "/-/pnpr/v0/compiler-cache/acme/a/b/cache-key";

#[tokio::test]
async fn parallel_uploads_are_rejected_before_buffering_and_cancellation_releases_capacity() {
    let directory = TempDir::new().unwrap();
    let app = app(config(&directory), "ci", false);
    let (started, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut uploads = Vec::new();
    for _ in 0..2 {
        let mut started = Some(started.clone());
        let body = Body::from_stream(futures_util::stream::poll_fn(
            move |_| -> std::task::Poll<Option<Result<Bytes, std::io::Error>>> {
                if let Some(started) = started.take() {
                    started.send(()).unwrap();
                }
                std::task::Poll::Pending
            },
        ));
        uploads.push(tokio::spawn(app.clone().oneshot(request(Method::PUT, ENTRY, body))));
    }
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        receiver.recv().await.unwrap();
        receiver.recv().await.unwrap();
    })
    .await
    .expect("both upload bodies must start reading");
    let body = Body::from_stream(futures_util::stream::poll_fn(
        |_| -> std::task::Poll<Option<Result<Bytes, std::io::Error>>> {
            panic!("overloaded upload body must not be read");
        },
    ));
    let rejected = app
        .clone()
        .oneshot(request(Method::PUT, &ENTRY.replace("/acme/", "/other/"), body))
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(rejected.headers()[header::RETRY_AFTER], "1");
    assert_eq!(rejected.headers()[header::CACHE_CONTROL], "private, no-store");
    let read = app.clone().oneshot(request(Method::GET, ENTRY, Body::empty())).await.unwrap();
    assert_eq!(read.status(), StatusCode::NOT_FOUND);
    for upload in uploads {
        upload.abort();
        assert!(upload.await.unwrap_err().is_cancelled(), "upload must be cancelled");
    }
    let published = app.oneshot(request(Method::PUT, ENTRY, Body::from("compiled"))).await.unwrap();
    assert_eq!(published.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn compiler_cache_limits_upload_size_and_rejects_invalid_keys() {
    let directory = TempDir::new().unwrap();
    let app = app(config(&directory), "ci", false);
    let response = app
        .clone()
        .oneshot(request(
            Method::PUT,
            ENTRY,
            Body::from(vec![0_u8; pnpr_shared_artifacts::MAX_COMPILER_CACHE_ENTRY_SIZE + 1]),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let response = app
        .oneshot(request(Method::PUT, "/-/pnpr/v0/compiler-cache/acme/a/%2e%2e/b", Body::empty()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

fn config(directory: &TempDir) -> Config {
    let path = directory.path().join("pnpr.yaml");
    std::fs::write(&path, "resolver:\n  enabled: false\nartifacts:\n  enabled: true\n  compilerCaches:\n    acme:\n      access: [ci, developer]\n      publish: ci\n    other:\n      access: ci\n      publish: ci\n").unwrap();
    Config::from_yaml(&path, "127.0.0.1:0".parse().unwrap(), None).unwrap()
}

fn app(config: Config, username: &str, readonly: bool) -> Router {
    let mut token = record(readonly, &[]);
    token.username = username.to_string();
    app_with_config_and_token(config, "token", token)
}

fn request(method: Method, path: &str, body: Body) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, "Bearer token")
        .body(body)
        .unwrap()
}

#[tokio::test]
async fn ci_publishes_and_developers_read_but_cannot_publish() {
    let directory = TempDir::new().unwrap();
    let config = config(&directory);
    let ci = app(config.clone(), "ci", false);
    let developer = app(config, "developer", false);
    let missing =
        developer.clone().oneshot(request(Method::GET, ENTRY, Body::empty())).await.unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let published =
        ci.clone().oneshot(request(Method::PUT, ENTRY, Body::from("compiled"))).await.unwrap();
    assert_eq!(published.status(), StatusCode::CREATED);
    let read = developer.clone().oneshot(request(Method::GET, ENTRY, Body::empty())).await.unwrap();
    assert_eq!(read.status(), StatusCode::OK);
    assert_eq!(read.headers()[header::CACHE_CONTROL], "private, no-store");
    assert_eq!(read.headers()[header::VARY], "Authorization");
    assert_eq!(to_bytes(read.into_body(), 100).await.unwrap(), "compiled");
    let head =
        developer.clone().oneshot(request(Method::HEAD, ENTRY, Body::empty())).await.unwrap();
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(head.headers()[header::CONTENT_LENGTH], "8");
    assert_eq!(to_bytes(head.into_body(), 100).await.unwrap().len(), 0);
    let denied =
        developer.oneshot(request(Method::PUT, ENTRY, Body::from("poison"))).await.unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let duplicate =
        ci.clone().oneshot(request(Method::PUT, ENTRY, Body::from("other bytes"))).await.unwrap();
    assert_eq!(duplicate.status(), StatusCode::OK);
    let read = ci.clone().oneshot(request(Method::GET, ENTRY, Body::empty())).await.unwrap();
    assert_eq!(to_bytes(read.into_body(), 100).await.unwrap(), "compiled");
    let other = ci
        .oneshot(request(Method::GET, &ENTRY.replace("/acme/", "/other/"), Body::empty()))
        .await
        .unwrap();
    assert_eq!(other.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unauthorized_or_readonly_publishers_are_rejected_before_reading_bodies() {
    let directory = TempDir::new().unwrap();
    let config = config(&directory);
    for (username, readonly, expected) in [
        ("stranger", false, StatusCode::NOT_FOUND),
        ("developer", false, StatusCode::FORBIDDEN),
        ("ci", true, StatusCode::FORBIDDEN),
    ] {
        let body = Body::from_stream(futures_util::stream::poll_fn(
            |_| -> std::task::Poll<Option<Result<Bytes, std::io::Error>>> {
                panic!("unauthorized body must not be read");
            },
        ));
        let response = app(config.clone(), username, readonly)
            .oneshot(request(Method::PUT, ENTRY, body))
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    let response = app(config.clone(), "ci", false)
        .oneshot(Request::get(ENTRY).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let mut revoked = request(Method::GET, ENTRY, Body::empty());
    revoked.headers_mut().insert(header::AUTHORIZATION, "Bearer revoked".parse().unwrap());
    let response = app(config, "ci", false).oneshot(revoked).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn disabled_artifacts_and_undeclared_caches_are_not_served() {
    let directory = TempDir::new().unwrap();
    let mut config = config(&directory);
    config.artifacts.compiler_caches.clear();
    assert_eq!(
        app(config.clone(), "ci", false)
            .oneshot(request(Method::GET, ENTRY, Body::empty()))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND,
    );
    config.artifacts.enabled = false;
    config.resolver.enabled = true;
    assert_eq!(
        app(config, "ci", false)
            .oneshot(request(Method::PUT, ENTRY, Body::empty()))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND,
    );
}
