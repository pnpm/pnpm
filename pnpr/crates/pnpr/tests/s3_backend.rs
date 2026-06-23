//! End-to-end test of the S3-backed hosted store, driven through the
//! real HTTP handlers. Uses `object_store`'s in-memory backend in place
//! of a real bucket, so it exercises the full publish (stage → upload)
//! and serve (stream-from-bucket) wiring without a network.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use futures_util::StreamExt;
use object_store::{ObjectStore, memory::InMemory, path::Path as ObjectPath};
use pnpr::{Config, HostedStoreConfig, router};
use serde_json::{Value, json};
use std::{
    fmt::Write,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
    sync::Arc,
};
use tempfile::TempDir;
use tower::ServiceExt;

/// A static-serve config whose hosted store is an in-memory object
/// store rather than the local `storage` directory.
fn s3_config(storage: PathBuf, store: Arc<dyn ObjectStore>) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::static_serve(listen, storage);
    config.public_url = "http://example.test".to_string();
    config.hosted_store = HostedStoreConfig::S3 { store, prefix: String::new() };
    config
}

#[tokio::test]
async fn publishes_to_and_serves_from_the_object_store() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let app = router(s3_config(storage.clone(), Arc::clone(&store)));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let bytes = b"fake-tarball-bytes";
    let body = sample_publish_body("mypkg", "1.0.0", bytes);
    let request = Request::put("/mypkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // The hosted store is the bucket, so nothing lands in the local
    // `storage` directory.
    assert!(!storage.join("mypkg").exists(), "hosted content must not touch local storage");

    // The packument round-trips out of the bucket, with the tarball URL
    // rewritten to the public URL.
    let response =
        app.clone().oneshot(Request::get("/mypkg").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let served = body_json(response.into_body()).await;
    assert_eq!(
        served["versions"]["1.0.0"]["dist"]["tarball"],
        "http://example.test/mypkg/-/mypkg-1.0.0.tgz",
    );

    // The tarball streams back byte-for-byte out of the bucket.
    let response = app
        .oneshot(Request::get("/mypkg/-/mypkg-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, bytes);
}

/// A publish whose tarball fails the integrity check must not leave an
/// object behind in the bucket, and must not leak the local staging
/// file it decoded into.
#[tokio::test]
async fn rejected_publish_uploads_nothing_and_leaves_no_staging_file() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let app = router(s3_config(storage.clone(), Arc::clone(&store)));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let mut body = sample_publish_body("bad-pkg", "1.0.0", b"actual-bytes");
    // Declare an integrity over different bytes than the body carries,
    // so the server's recomputed hash won't match.
    body["versions"]["1.0.0"]["dist"]["integrity"] = json!(sri_sha512(b"different-bytes"));
    let request = Request::put("/bad-pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let text = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(text.contains("EINTEGRITY"), "error body should carry EINTEGRITY: {text}");

    assert!(bucket_keys(&store, "bad-pkg").await.is_empty(), "nothing should be uploaded");
    assert_eq!(staging_file_count(&storage), 0, "no staging tmp file should be left behind");
}

/// A full-package unpublish (`DELETE /:pkg/-rev/:rev`) must remove the
/// package's objects from the bucket.
#[tokio::test]
async fn unpublish_removes_the_package_from_the_bucket() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let app = router(s3_config(storage.clone(), Arc::clone(&store)));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let body = sample_publish_body("mypkg", "1.0.0", b"fake-tarball-bytes");
    let request = Request::put("/mypkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);
    assert_eq!(bucket_keys(&store, "mypkg").await.len(), 2, "packument + tarball uploaded");

    let request = Request::delete("/mypkg/-rev/anything")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.oneshot(request).await.unwrap().status(), StatusCode::CREATED);

    assert!(
        bucket_keys(&store, "mypkg").await.is_empty(),
        "package should be gone from the bucket",
    );
}

/// Every object key in the bucket under `<prefix>/`.
async fn bucket_keys(store: &Arc<dyn ObjectStore>, prefix: &str) -> Vec<String> {
    let scope = ObjectPath::from(prefix);
    store
        .list(Some(&scope))
        .map(|meta| meta.expect("list entry").location.to_string())
        .collect::<Vec<_>>()
        .await
}

/// Number of files left in the S3 upload-staging directory (a
/// subdirectory of the proxy-cache root, which `static_serve` nests
/// under `storage`).
fn staging_file_count(storage: &Path) -> usize {
    let dir = storage.join(".pnpr-cache").join("pnpr-hosted-staging");
    std::fs::read_dir(dir).map_or(0, std::iter::Iterator::count)
}

async fn body_bytes(body: Body) -> Vec<u8> {
    to_bytes(body, usize::MAX).await.expect("read body").to_vec()
}

async fn body_json(body: Body) -> Value {
    serde_json::from_slice(&body_bytes(body).await).expect("body parses as JSON")
}

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
    let request = Request::put(&path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let payload = body_json(response.into_body()).await;
    let token = payload["token"].as_str().expect("token in response").to_string();
    (app, token)
}

fn sample_publish_body(name: &str, version: &str, tarball: &[u8]) -> Value {
    let filename = format!("{name}-{version}.tgz");
    json!({
        "_id": name,
        "name": name,
        "description": "test",
        "dist-tags": { "latest": version },
        "versions": {
            version: {
                "name": name,
                "version": version,
                "dist": {
                    "tarball": format!("http://localhost:4873/{name}/-/{filename}"),
                    "shasum": sha1_hex(tarball),
                    "integrity": sri_sha512(tarball),
                }
            }
        },
        "_attachments": {
            filename: {
                "content_type": "application/octet-stream",
                "data": BASE64.encode(tarball),
                "length": tarball.len()
            }
        }
    })
}

fn sri_sha512(bytes: &[u8]) -> String {
    let mut opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    opts.input(bytes);
    opts.result().to_string()
}

fn sha1_hex(bytes: &[u8]) -> String {
    let mut opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha1);
    opts.input(bytes);
    let integrity = opts.result();
    let digest_bytes = BASE64.decode(&integrity.hashes[0].digest).unwrap();
    digest_bytes.iter().fold(String::with_capacity(40), |mut acc, byte| {
        write!(acc, "{byte:02x}").unwrap();
        acc
    })
}
