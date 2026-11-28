use crate::ecosystem::{
    body_bytes,
    sha256_hex,
};
use axum::{
    Router,
    body::{
        Body,
        to_bytes,
    },
    http::{
        Request,
        StatusCode,
    },
    response::Response,
};
use pnpr::{
    AuthState,
    router_with_auth,
};
use tempfile::TempDir;
use tower::ServiceExt;

fn registry(root: &TempDir, origin: &str) -> Router {
    let path = root.path().join("config.yaml");
    std::fs::write(&path, format!("storage: ./storage\nregistries:\n  runtimes: {{type: upstream, ecosystem: node, url: {origin}/dist/, public: true}}\ndefaultRegistry: {{node: runtimes}}\n")).unwrap();
    let mut config = pnpr_fixtures::load_seeded_config(
        &path,
        "127.0.0.1:4873".parse().unwrap(),
        Some(crate::ecosystem::PUBLIC_URL.to_string()),
    );
    config.routing.route_policy.allowed_private_networks = crate::network::loopback_networks();
    router_with_auth(config, AuthState::in_memory())
}

async fn get(app: &Router, path: &str) -> Response {
    app.clone()
        .oneshot(
            Request::get(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn serves_release_files_through_default_and_named_registries() {
    let mut origin = mockito::Server::new_async().await;
    let tmp = TempDir::new().unwrap();
    let app = registry(&tmp, &origin.url());
    let filename = "node-v22.0.0-headers.tar.gz";
    let sums = format!(
        "{}  {filename}\n{}  win-x64/node.lib\n{}  win-arm64/node.lib\n",
        sha256_hex(b"headers"),
        sha256_hex(b"x64 library"),
        sha256_hex(b"arm64 library"),
    );
    for (path, body) in [
        ("index.json".to_string(), "[]"),
        ("v22.0.0/SHASUMS256.txt".to_string(), sums.as_str()),
        ("v22.0.0/SHASUMS256.txt.sig".to_string(), "signature"),
        (format!("v22.0.0/{filename}"), "headers"),
        ("v22.0.0/win-x64/node.lib".to_string(), "x64 library"),
        ("v22.0.0/win-arm64/node.lib".to_string(), "arm64 library"),
    ] {
        let mock = origin
            .mock("GET", format!("/dist/{path}").as_str())
            .with_body(body)
            .expect(1)
            .create_async()
            .await;
        for base in ["/node/release", "/node/~runtimes/release"] {
            let response = get(&app, &format!("{base}/{path}")).await;
            let status = response.status();
            let bytes = body_bytes(response.into_body()).await;
            assert_eq!(status, StatusCode::OK, "{path}: {}", String::from_utf8_lossy(&bytes));
            assert_eq!(bytes, body.as_bytes());
        }
        mock.assert_async().await;
    }
    let x64 = get(&app, "/node/release/v22.0.0/win-x64/node.lib").await;
    assert_eq!(body_bytes(x64.into_body()).await, b"x64 library");
    let missing = get(&app, "/node/release/v22.0.0/node-v22.0.0-linux-arm64.tar.gz").await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn checksum_mismatch_leaves_the_runtime_download_retryable() {
    let mut origin = mockito::Server::new_async().await;
    let tmp = TempDir::new().unwrap();
    let app = registry(&tmp, &origin.url());
    let filename = "node-v22.0.0-linux-x64.tar.gz";
    let _sums = origin
        .mock("GET", "/dist/v22.0.0/SHASUMS256.txt")
        .with_body(format!("{}  {filename}\n", sha256_hex(b"runtime")))
        .create_async()
        .await;
    let path = format!("/dist/v22.0.0/{filename}");
    let route = format!("/node/release/v22.0.0/{filename}");
    let bad = origin
        .mock("GET", path.as_str())
        .with_body("incorrect")
        .create_async()
        .await;
    assert!(to_bytes(get(&app, &route).await.into_body(), 1024).await.is_err());
    bad.remove_async().await;
    let _good = origin
        .mock("GET", path.as_str())
        .with_body("runtime")
        .create_async()
        .await;
    assert_eq!(body_bytes(get(&app, &route).await.into_body()).await, b"runtime");
}
