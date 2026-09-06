//! Integration tests for `PUT /-/pnpm/v1/publish` — the batch
//! publish endpoint `pnpm publish --batch` talks to. Static-mode (no
//! upstream) to keep the tests hermetic.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
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

async fn body_bytes(body: Body) -> Vec<u8> {
    to_bytes(body, usize::MAX).await.expect("read body").to_vec()
}

async fn body_json(body: Body) -> Value {
    serde_json::from_slice(&body_bytes(body).await).expect("body parses as JSON")
}

fn put_json(path: &str, body: &Value) -> Request<Body> {
    Request::put(path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

fn put_json_with_token(path: &str, body: &Value, token: &str) -> Request<Body> {
    Request::put(path)
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

async fn add_user_and_get_token(app: axum::Router, username: &str, password: &str) -> String {
    let path = format!("/-/user/org.couchdb.user:{username}");
    let body = json!({
        "_id": format!("org.couchdb.user:{username}"),
        "name": username,
        "password": password,
        "email": "foo@bar.net",
        "type": "user",
        "roles": [],
    });
    let response = app.oneshot(put_json(&path, &body)).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let payload = body_json(response.into_body()).await;
    payload["token"].as_str().expect("token in response").to_string()
}

fn publish_doc(name: &str, version: &str, tarball: &[u8]) -> Value {
    let basename = name.rsplit('/').next().unwrap_or(name);
    let filename = format!("{basename}-{version}.tgz");
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

#[tokio::test]
async fn batch_publish_writes_every_package_in_one_request() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;

    let bytes_a = b"tarball-a";
    let bytes_b = b"tarball-b";
    let body = json!({
        "packages": [
            publish_doc("batch-a", "1.0.0", bytes_a),
            publish_doc("batch-b", "2.0.0", bytes_b),
        ],
    });
    let response = app
        .clone()
        .oneshot(put_json_with_token("/-/pnpm/v1/publish", &body, &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let payload = body_json(response.into_body()).await;
    assert_eq!(payload["ok"], true);

    for (name, version, bytes) in
        [("batch-a", "1.0.0", bytes_a.as_slice()), ("batch-b", "2.0.0", bytes_b.as_slice())]
    {
        let packument: Value = serde_json::from_slice(
            &std::fs::read(storage.join(name).join("package.json")).expect("packument written"),
        )
        .unwrap();
        assert_eq!(packument["name"], name);
        assert_eq!(packument["versions"][version]["version"], version);
        assert_eq!(packument["dist-tags"]["latest"], version);
        assert!(packument.get("_attachments").is_none(), "_attachments should not be persisted");

        let on_disk_tarball =
            std::fs::read(storage.join(name).join(format!("{name}-{version}.tgz")))
                .expect("tarball written");
        assert_eq!(on_disk_tarball, bytes);

        let served = app
            .clone()
            .oneshot(Request::get(format!("/{name}")).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(served.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn batch_publish_supports_scoped_packages_with_libnpmpublish_attachment_names() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;

    let bytes = b"scoped-batch-bytes";
    // `pnpm publish --batch` names attachments by the full scoped name
    // + version, same as libnpmpublish: `@scope/name-version.tgz`. The
    // submitted `dist.tarball` uses that scoped filename too — the
    // 5-segment form every libnpmpublish-based client sends.
    let body = json!({
        "packages": [{
            "_id": "@scope/wire-form",
            "name": "@scope/wire-form",
            "dist-tags": { "latest": "1.0.0" },
            "versions": {
                "1.0.0": {
                    "name": "@scope/wire-form",
                    "version": "1.0.0",
                    "dist": {
                        "tarball": "http://localhost:4873/@scope/wire-form/-/@scope/wire-form-1.0.0.tgz",
                        "shasum": sha1_hex(bytes),
                        "integrity": sri_sha512(bytes),
                    },
                },
            },
            "_attachments": {
                "@scope/wire-form-1.0.0.tgz": {
                    "content_type": "application/octet-stream",
                    "data": BASE64.encode(bytes),
                    "length": bytes.len(),
                },
            },
        }],
    });
    let response = app
        .clone()
        .oneshot(put_json_with_token("/-/pnpm/v1/publish", &body, &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // On disk: canonical `<basename>-<version>.tgz` under the scope dir.
    let on_disk = storage.join("@scope/wire-form/wire-form-1.0.0.tgz");
    assert_eq!(std::fs::read(&on_disk).unwrap(), bytes);

    // The served packument canonicalizes `dist.tarball` to the
    // routable 4-segment form, regardless of the scoped filename the
    // client submitted.
    let packument_response = app
        .clone()
        .oneshot(Request::get("/@scope/wire-form").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(packument_response.status(), StatusCode::OK);
    let served_packument = body_json(packument_response.into_body()).await;
    assert_eq!(
        served_packument["versions"]["1.0.0"]["dist"]["tarball"],
        "http://example.test/@scope/wire-form/-/wire-form-1.0.0.tgz",
    );

    let served = app
        .oneshot(
            Request::get("/@scope/wire-form/-/wire-form-1.0.0.tgz").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(served.status(), StatusCode::OK);
    assert_eq!(body_bytes(served.into_body()).await, bytes);
}

#[tokio::test]
async fn anonymous_batch_publish_is_rejected() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));

    let body = json!({ "packages": [publish_doc("anon-batch", "1.0.0", b"bytes")] });
    let response = app.oneshot(put_json("/-/pnpm/v1/publish", &body)).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(!storage.join("anon-batch").exists());
}

/// The batch is all-or-nothing: when one package's tarball fails the
/// integrity check, packages staged before it must not become
/// visible — no packument, no tarball, no `*.tmp.*` leftovers.
#[tokio::test]
async fn batch_publish_rolls_back_every_package_when_one_fails_integrity() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;

    let good = publish_doc("rollback-good", "1.0.0", b"good-bytes");
    let mut bad = publish_doc("rollback-bad", "1.0.0", b"actual-bytes");
    bad["versions"]["1.0.0"]["dist"]["integrity"] = json!(sri_sha512(b"different-bytes"));

    let body = json!({ "packages": [good, bad] });
    let response =
        app.oneshot(put_json_with_token("/-/pnpm/v1/publish", &body, &token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body_text = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(body_text.contains("EINTEGRITY"), "error should carry EINTEGRITY: {body_text}");

    for name in ["rollback-good", "rollback-bad"] {
        let pkg_dir = storage.join(name);
        if pkg_dir.exists() {
            let entries: Vec<String> = std::fs::read_dir(&pkg_dir)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
                .collect();
            assert!(
                entries.is_empty(),
                "expected no artifacts for {name} after rejected batch, found: {entries:?}",
            );
        }
    }
}

#[tokio::test]
async fn batch_publish_rejects_duplicate_package_names() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;

    let body = json!({
        "packages": [
            publish_doc("dupe-pkg", "1.0.0", b"v1"),
            publish_doc("dupe-pkg", "2.0.0", b"v2"),
        ],
    });
    let response =
        app.oneshot(put_json_with_token("/-/pnpm/v1/publish", &body, &token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body_text = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(body_text.contains("duplicate package"), "got: {body_text}");
    assert!(!storage.join("dupe-pkg").exists());
}

#[tokio::test]
async fn batch_publish_rejects_bodies_without_a_packages_array() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;

    for body in [json!({}), json!({ "packages": [] }), json!({ "packages": "nope" }), json!([])] {
        let response = app
            .clone()
            .oneshot(put_json_with_token("/-/pnpm/v1/publish", &body, &token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "body: {body}");
    }
}

#[tokio::test]
async fn batch_publish_rejects_entries_without_a_name() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;

    let body = json!({ "packages": [{ "versions": {} }] });
    let response =
        app.oneshot(put_json_with_token("/-/pnpm/v1/publish", &body, &token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// A batched publish merges into existing packuments the same way a
/// single-package publish does: earlier versions survive.
#[tokio::test]
async fn batch_publish_merges_with_previously_published_versions() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;

    let first = json!({ "packages": [publish_doc("merge-pkg", "1.0.0", b"v1")] });
    let response = app
        .clone()
        .oneshot(put_json_with_token("/-/pnpm/v1/publish", &first, &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let second = json!({ "packages": [publish_doc("merge-pkg", "2.0.0", b"v2")] });
    let response =
        app.oneshot(put_json_with_token("/-/pnpm/v1/publish", &second, &token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let packument: Value =
        serde_json::from_slice(&std::fs::read(storage.join("merge-pkg/package.json")).unwrap())
            .unwrap();
    assert_eq!(packument["versions"]["1.0.0"]["version"], "1.0.0");
    assert_eq!(packument["versions"]["2.0.0"]["version"], "2.0.0");
    assert_eq!(packument["dist-tags"]["latest"], "2.0.0");
}

/// Compute the SRI `sha512-...` string the way npm clients send it
/// in `dist.integrity`.
fn sri_sha512(bytes: &[u8]) -> String {
    let mut opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    opts.input(bytes);
    opts.result().to_string()
}

/// Compute the 40-char hex SHA-1 the way npm clients send it in the
/// legacy `dist.shasum` field.
fn sha1_hex(bytes: &[u8]) -> String {
    let mut opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha1);
    opts.input(bytes);
    let integrity = opts.result();
    let digest_base64 = &integrity.hashes[0].digest;
    let digest_bytes = BASE64.decode(digest_base64).unwrap();
    digest_bytes.iter().fold(String::with_capacity(40), |mut acc, byte| {
        use std::fmt::Write;
        write!(acc, "{byte:02x}").unwrap();
        acc
    })
}
