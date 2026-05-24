//! End-to-end tests for static-serve mode, sourcing the storage
//! directory from `@pnpm/registry-mock`'s installed copy in the
//! workspace. `@pnpm/registry-mock`'s published npm tarball ships a
//! prepared verdaccio `storage-cache/` (scoped packages under
//! `@foo`, `@pnpm.e2e`, etc.); this exercise asserts that
//! pnpm-registry serves it correctly without any upstream proxy.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use pipe_trait::Pipe;
use serde_json::Value;
use tower::ServiceExt;

use pnpm_registry::{Config, router};

const PUBLIC_URL: &str = "http://example.test";

fn workspace_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        Command::new(env!("CARGO"))
            .arg("locate-project")
            .arg("--workspace")
            .arg("--message-format=plain")
            .output()
            .expect("cargo locate-project")
            .stdout
            .pipe(String::from_utf8)
            .expect("utf8 stdout")
            .trim_end()
            .pipe(Path::new)
            .parent()
            .expect("parent of root manifest")
            .to_path_buf()
    })
}

fn registry_mock_storage() -> PathBuf {
    workspace_root()
        .join("pacquet/tasks/registry-mock/node_modules/@pnpm/registry-mock/registry/storage-cache")
}

fn static_config(storage: PathBuf) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::static_serve(listen, storage);
    config.public_url = PUBLIC_URL.to_string();
    config
}

async fn body_bytes(body: Body) -> Vec<u8> {
    to_bytes(body, usize::MAX).await.expect("read body").to_vec()
}

#[tokio::test]
async fn serves_scoped_packument_from_registry_mock_storage() {
    let storage = registry_mock_storage();
    assert!(
        storage.join("@foo/no-deps/package.json").exists(),
        "registry-mock storage is not populated at {storage:?} — run `pnpm install` first",
    );

    let app = router(static_config(storage));

    let response =
        app.oneshot(Request::get("/@foo/no-deps").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let doc: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(doc["name"], "@foo/no-deps");
    assert_eq!(doc["dist-tags"]["latest"], "1.0.0");

    // The on-disk packument has a Verdaccio-form tarball URL —
    // `http://localhost:4873/@foo/no-deps/-/@foo/no-deps-1.0.0.tgz`.
    // After our rewrite the client should see the npm-spec form
    // pointed at our `public_url`.
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["tarball"],
        format!("{PUBLIC_URL}/@foo/no-deps/-/no-deps-1.0.0.tgz"),
    );
    // Other fields (integrity, shasum, name, version) should pass
    // through untouched.
    assert_eq!(
        doc["versions"]["1.0.0"]["dist"]["shasum"],
        "30909ad03bbccde8929f516e4644a62cf7f82785",
    );
    assert!(doc["versions"]["1.0.0"]["dist"]["integrity"].as_str().unwrap().starts_with("sha512-"));
}

#[tokio::test]
async fn serves_scoped_tarball_from_registry_mock_storage() {
    let storage = registry_mock_storage();
    let on_disk = storage.join("@foo/no-deps/no-deps-1.0.0.tgz");
    let expected_bytes = std::fs::read(&on_disk).expect("registry-mock tarball");

    let app = router(static_config(storage));

    let response = app
        .oneshot(Request::get("/@foo/no-deps/-/no-deps-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let received = body_bytes(response.into_body()).await;
    assert_eq!(received, expected_bytes);
}

#[tokio::test]
async fn static_mode_returns_404_for_unknown_package() {
    let app = router(static_config(registry_mock_storage()));

    let response = app
        .oneshot(Request::get("/@foo/this-package-does-not-exist").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn serves_version_manifest_by_dist_tag() {
    let storage = registry_mock_storage();
    let app = router(static_config(storage));

    let response = app
        .oneshot(Request::get("/@foo/no-deps/latest").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let manifest: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(manifest["name"], "@foo/no-deps");
    assert_eq!(manifest["version"], "1.0.0");
    assert_eq!(
        manifest["dist"]["tarball"],
        format!("{PUBLIC_URL}/@foo/no-deps/-/no-deps-1.0.0.tgz"),
    );
    // The response is the single-version manifest, not the whole
    // packument — `versions` shouldn't be there.
    assert!(manifest.get("versions").is_none());
}

#[tokio::test]
async fn serves_version_manifest_by_literal_version() {
    let storage = registry_mock_storage();
    let app = router(static_config(storage));

    let response = app
        .oneshot(Request::get("/@foo/no-deps/1.0.0").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let manifest: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(manifest["version"], "1.0.0");
}

#[tokio::test]
async fn version_manifest_returns_404_for_unknown_version() {
    let storage = registry_mock_storage();
    let app = router(static_config(storage));

    let response = app
        .oneshot(Request::get("/@foo/no-deps/99.0.0").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn static_mode_returns_404_for_unknown_tarball() {
    let app = router(static_config(registry_mock_storage()));

    let response = app
        .oneshot(Request::get("/@foo/no-deps/-/no-deps-99.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
