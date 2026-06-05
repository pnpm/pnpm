//! Integration tests for the auth, dist-tag, and publish endpoints.
//! Static-mode (no upstream) to keep the tests hermetic.

// `#[path]` rather than the `tests/common/mod.rs` layout, which the
// Perfectionist dylint forbids.
#[path = "common/storage.rs"]
mod common;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use pnpr::{Config, router};
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
    config
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

#[tokio::test]
async fn adduser_creates_user_and_returns_token() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (_, token) = add_user_and_get_token(app, "alice", "secret").await;
    assert!(!token.is_empty(), "token should be non-empty");
}

#[tokio::test]
async fn adduser_returns_token_on_repeat_login() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, first) = add_user_and_get_token(app, "alice", "secret").await;
    let (_, second) = add_user_and_get_token(app, "alice", "secret").await;
    assert!(!first.is_empty() && !second.is_empty());
    // We mint a fresh token each call. The important property is
    // that *both* tokens work, not that they're identical.
    assert_ne!(first, second);
}

#[tokio::test]
async fn adduser_rejects_wrong_password_for_existing_user() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, _) = add_user_and_get_token(app, "alice", "secret").await;

    let path = "/-/user/org.couchdb.user:alice";
    let body = json!({
        "name": "alice", "password": "wrong",
        "email": "foo@bar.net", "type": "user", "roles": []
    });
    let response = app.oneshot(put_json(path, body)).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn anonymous_request_to_protected_package_returns_401() {
    let storage = common::build_storage();
    let app = router(static_config(storage.path().to_path_buf()));
    // The fixture publishes @pnpm.e2e/needs-auth — but our access
    // policy still requires auth for it because the package name
    // matches the `@pnpm.e2e/needs-auth` policy rule.
    let response = app
        .oneshot(Request::get("/@pnpm.e2e/needs-auth").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn bearer_token_grants_access_to_protected_package() {
    let storage = common::build_storage();
    let app = router(static_config(storage.path().to_path_buf()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let response = app
        .oneshot(
            Request::get("/@pnpm.e2e/needs-auth")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn basic_auth_grants_access_to_protected_package() {
    let storage = common::build_storage();
    let app = router(static_config(storage.path().to_path_buf()));
    let (app, _) = add_user_and_get_token(app, "alice", "secret").await;

    let basic = BASE64.encode(b"alice:secret");
    let response = app
        .oneshot(
            Request::get("/@pnpm.e2e/needs-auth")
                .header("Authorization", format!("Basic {basic}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn anonymous_publish_is_rejected() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let body = sample_publish_body("anon-test", "1.0.0", b"tarball-bytes");
    let response = app.oneshot(put_json("/anon-test", body)).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn authenticated_publish_writes_manifest_and_tarball() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
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

    // Packument on disk
    let on_disk_packument =
        std::fs::read(storage.join("mypkg/package.json")).expect("packument written");
    let packument: Value = serde_json::from_slice(&on_disk_packument).unwrap();
    assert_eq!(packument["name"], "mypkg");
    assert_eq!(packument["versions"]["1.0.0"]["version"], "1.0.0");
    assert_eq!(packument["dist-tags"]["latest"], "1.0.0");
    assert!(packument.get("_attachments").is_none(), "_attachments should not be persisted");

    // Tarball on disk
    let on_disk_tarball =
        std::fs::read(storage.join("mypkg/mypkg-1.0.0.tgz")).expect("tarball written");
    assert_eq!(on_disk_tarball, bytes);

    // Subsequent GET serves it back with the public URL rewritten.
    let response = app.oneshot(Request::get("/mypkg").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let served = body_json(response.into_body()).await;
    assert_eq!(
        served["versions"]["1.0.0"]["dist"]["tarball"],
        "http://example.test/mypkg/-/mypkg-1.0.0.tgz",
    );
}

/// Published packages are the source of truth: they live in the
/// authoritative `storage` root, never in the disposable proxy cache,
/// and survive a full wipe of that cache.
#[tokio::test]
async fn published_package_survives_wiping_the_proxy_cache() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let bytes = b"durable-tarball-bytes";
    let body = sample_publish_body("durable-pkg", "1.0.0", bytes);
    let request = Request::put("/durable-pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);

    // The artifacts land in the authoritative root, not the cache.
    assert!(storage.join("durable-pkg/package.json").exists());
    assert!(storage.join("durable-pkg/durable-pkg-1.0.0.tgz").exists());
    assert!(
        !storage.join(".pnpr-cache/durable-pkg").exists(),
        "published package must not be written into the disposable proxy cache",
    );

    // Blow away the entire proxy cache, the way an operator reclaiming
    // disk (or a fresh container on an ephemeral cache volume) would.
    let cache_root = storage.join(".pnpr-cache");
    std::fs::create_dir_all(&cache_root).unwrap();
    std::fs::remove_dir_all(&cache_root).unwrap();

    // The package is still served, tarball and all.
    let response = app
        .clone()
        .oneshot(Request::get("/durable-pkg").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let served = body_json(response.into_body()).await;
    assert_eq!(served["versions"]["1.0.0"]["version"], "1.0.0");

    let response = app
        .oneshot(Request::get("/durable-pkg/-/durable-pkg-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, bytes);
}

/// When the same tarball filename exists in both stores, `open_tarball`
/// serves the hosted copy — a stale proxied copy can't shadow it.
#[tokio::test]
async fn hosted_tarball_is_preferred_over_a_cached_copy() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let hosted_bytes = b"hosted-bytes";
    let body = sample_publish_body("pref-pkg", "1.0.0", hosted_bytes);
    let request = Request::put("/pref-pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);

    // Plant a divergent proxied copy with the same filename.
    let cached = storage.join(".pnpr-cache").join("pref-pkg");
    std::fs::create_dir_all(&cached).unwrap();
    std::fs::write(cached.join("pref-pkg-1.0.0.tgz"), b"stale-proxied-bytes").unwrap();

    let response = app
        .oneshot(Request::get("/pref-pkg/-/pref-pkg-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, hosted_bytes);
}

#[tokio::test]
async fn publish_followed_by_dist_tag_set_works() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    // First publish 1.0.0
    let body = sample_publish_body("tagpkg", "1.0.0", b"v1");
    let request = Request::put("/tagpkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    app.clone().oneshot(request).await.unwrap();

    // Then publish 2.0.0 (without changing latest)
    let mut body = sample_publish_body("tagpkg", "2.0.0", b"v2");
    body["dist-tags"] = json!({}); // don't bump latest
    let request = Request::put("/tagpkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    app.clone().oneshot(request).await.unwrap();

    // Confirm latest is still 1.0.0.
    let tags = app
        .clone()
        .oneshot(Request::get("/-/package/tagpkg/dist-tags").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(tags.status(), StatusCode::OK);
    let tags_body = body_json(tags.into_body()).await;
    assert_eq!(tags_body["latest"], "1.0.0");

    // PUT a new "beta" tag pointing at 2.0.0.
    let request = Request::put("/-/package/tagpkg/dist-tags/beta")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_string("2.0.0").unwrap()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // Confirm the tag landed.
    let tags = app
        .clone()
        .oneshot(Request::get("/-/package/tagpkg/dist-tags").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let tags_body = body_json(tags.into_body()).await;
    assert_eq!(tags_body["beta"], "2.0.0");
    assert_eq!(tags_body["latest"], "1.0.0");

    // And via the version-manifest endpoint resolving the tag.
    let manifest = app
        .clone()
        .oneshot(Request::get("/tagpkg/beta").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let manifest_body = body_json(manifest.into_body()).await;
    assert_eq!(manifest_body["version"], "2.0.0");

    // Now DELETE it.
    let request = Request::delete("/-/package/tagpkg/dist-tags/beta")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let tags = app
        .oneshot(Request::get("/-/package/tagpkg/dist-tags").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let tags_body = body_json(tags.into_body()).await;
    assert!(tags_body.get("beta").is_none(), "beta tag should be removed");
}

#[tokio::test]
async fn dist_tag_mutations_refresh_time_modified() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let body = sample_publish_body("time-mod-pkg", "1.0.0", b"x");
    let request = Request::put("/time-mod-pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    app.clone().oneshot(request).await.unwrap();

    let initial_time = serde_json::from_slice::<Value>(
        &std::fs::read(storage.join("time-mod-pkg/package.json")).unwrap(),
    )
    .unwrap()["time"]["modified"]
        .as_str()
        .expect("modified is a string")
        .to_string();

    // Wait long enough that ISO-millisecond timestamps will differ.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;

    let request = Request::put("/-/package/time-mod-pkg/dist-tags/next")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_string("1.0.0").unwrap()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let after_set = serde_json::from_slice::<Value>(
        &std::fs::read(storage.join("time-mod-pkg/package.json")).unwrap(),
    )
    .unwrap()["time"]["modified"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(initial_time, after_set, "dist-tag PUT should bump time.modified");

    tokio::time::sleep(std::time::Duration::from_millis(5)).await;

    let request = Request::delete("/-/package/time-mod-pkg/dist-tags/next")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let after_delete = serde_json::from_slice::<Value>(
        &std::fs::read(storage.join("time-mod-pkg/package.json")).unwrap(),
    )
    .unwrap()["time"]["modified"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(after_set, after_delete, "dist-tag DELETE should bump time.modified too");
}

#[tokio::test]
async fn dist_tag_set_requires_auth() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let request = Request::put("/-/package/anything/dist-tags/latest")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string("1.0.0").unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn publish_rejects_body_name_that_doesnt_match_url() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let body = sample_publish_body("other-name", "1.0.0", b"x");
    let request = Request::put("/url-name")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn publish_rejects_tarball_that_doesnt_match_package() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let bytes = b"tarball-bytes";
    let mut body = sample_publish_body("foo", "1.0.0", bytes);
    // Override _attachments to use a filename for a different package
    body["_attachments"] = json!({
        "bar-1.0.0.tgz": {
            "content_type": "application/octet-stream",
            "data": BASE64.encode(bytes),
            "length": bytes.len()
        }
    });
    let request = Request::put("/foo")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn publish_rejects_integrity_mismatch_and_leaves_no_artifacts() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let bytes = b"actual-bytes";
    let mut body = sample_publish_body("bad-pkg", "1.0.0", bytes);
    // Swap the integrity for one computed over different bytes — the
    // body keeps the original bytes, so the server's recomputed hash
    // won't match.
    body["versions"]["1.0.0"]["dist"]["integrity"] = json!(sri_sha512(b"different-bytes"));

    let request = Request::put("/bad-pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body_text = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(
        body_text.contains("EINTEGRITY"),
        "error body should carry EINTEGRITY code: {body_text}",
    );

    // Neither the packument nor the tarball should have been written.
    assert!(
        !storage.join("bad-pkg/package.json").exists(),
        "packument must not be written when integrity check fails",
    );
    assert!(
        !storage.join("bad-pkg/bad-pkg-1.0.0.tgz").exists(),
        "tarball must not be written when integrity check fails",
    );
}

#[tokio::test]
async fn publish_rejects_shasum_mismatch() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let bytes = b"shasum-test-bytes";
    let mut body = sample_publish_body("shasum-pkg", "1.0.0", bytes);
    // Keep integrity valid but corrupt the legacy shasum.
    body["versions"]["1.0.0"]["dist"]["shasum"] = json!("0000000000000000000000000000000000000000");

    let request = Request::put("/shasum-pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body_text = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(
        body_text.contains("EINTEGRITY"),
        "shasum mismatch must surface EINTEGRITY: {body_text}",
    );
    assert!(!storage.join("shasum-pkg/package.json").exists());
}

#[tokio::test]
async fn publish_rejects_missing_integrity_field() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let bytes = b"no-integrity-bytes";
    let mut body = sample_publish_body("no-int-pkg", "1.0.0", bytes);
    body["versions"]["1.0.0"]["dist"].as_object_mut().unwrap().remove("integrity");

    let request = Request::put("/no-int-pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body_text = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(
        body_text.contains("EINTEGRITY") && body_text.contains("integrity"),
        "missing integrity must surface a clear EINTEGRITY message: {body_text}",
    );
    assert!(!storage.join("no-int-pkg/package.json").exists());
}

/// Real npm clients send one attachment per publish, but the
/// handler is written to iterate N: if the M-th attachment fails
/// integrity, every already-written tmp file from the earlier
/// attachments must be cleaned up so a rejected publish leaves no
/// on-disk artifact. This pins down the `cleanup_tmp_slots` path —
/// without it, a regression that no-op'd the cleanup would leak
/// `*.tmp.*` files for every successful attachment before the bad
/// one.
#[tokio::test]
async fn publish_with_failed_attachment_cleans_up_earlier_tmp_files() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let bytes_v1 = b"valid-first-attachment";
    let bytes_v2 = b"second-attachment";
    let body = json!({
        "_id": "multi-attach",
        "name": "multi-attach",
        "dist-tags": { "latest": "2.0.0" },
        "versions": {
            "1.0.0": {
                "name": "multi-attach", "version": "1.0.0",
                "dist": {
                    "tarball": "http://localhost:4873/multi-attach/-/multi-attach-1.0.0.tgz",
                    "shasum": sha1_hex(bytes_v1),
                    "integrity": sri_sha512(bytes_v1),
                }
            },
            "2.0.0": {
                "name": "multi-attach", "version": "2.0.0",
                "dist": {
                    "tarball": "http://localhost:4873/multi-attach/-/multi-attach-2.0.0.tgz",
                    "shasum": sha1_hex(bytes_v2),
                    "integrity": sri_sha512(b"WRONG-BYTES-FOR-2.0.0"),
                }
            }
        },
        "_attachments": {
            "multi-attach-1.0.0.tgz": {
                "content_type": "application/octet-stream",
                "data": BASE64.encode(bytes_v1),
                "length": bytes_v1.len(),
            },
            "multi-attach-2.0.0.tgz": {
                "content_type": "application/octet-stream",
                "data": BASE64.encode(bytes_v2),
                "length": bytes_v2.len(),
            },
        }
    });
    let request = Request::put("/multi-attach")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // The package dir may exist (the handler creates it while
    // reserving paths) but it must be empty: no .tgz files, no
    // .tmp.* leftovers, no package.json.
    let pkg_dir = storage.join("multi-attach");
    if pkg_dir.exists() {
        let entries: Vec<String> = std::fs::read_dir(&pkg_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            entries.is_empty(),
            "expected no artifacts after rejected publish, found: {entries:?}",
        );
    }
}

/// `anonymous-npm-registry-client`'s `distTags.add` URL-encodes the
/// `/` in scoped names: `@scope/pkg` → `@scope%2Fpkg` in the URL.
/// axum's `Path` extractor percent-decodes path segments, so the
/// handler sees the decoded value verbatim and never needs to decode
/// again. This regression test pins that down: if someone reintroduces
/// a manual `urldecode` (which was previously here and was both
/// redundant and buggy on literal `%` chars), the `@scope/pkg`
/// `PackageName::parse` would still pass but a future bug-fix that
/// changes the decoder could break percent-encoded scoped paths.
#[tokio::test]
async fn dist_tag_set_works_with_url_encoded_scoped_path() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    // Publish @scope/pkg@1.0.0 (literal slash in the URL — pnpm
    // publish uses this form), then set a dist-tag using the
    // npm-client-style `%2F` encoding to verify the decoded path
    // reaches the handler.
    let body = sample_publish_body("@scope/pkg", "1.0.0", b"v1");
    let request = Request::put("/@scope/pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    app.clone().oneshot(request).await.unwrap();

    let request = Request::put("/-/package/@scope%2Fpkg/dist-tags/beta")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_string("1.0.0").unwrap()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // Fetch via the encoded form too — both should round-trip cleanly.
    let tags = app
        .oneshot(Request::get("/-/package/@scope%2Fpkg/dist-tags").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(tags.status(), StatusCode::OK);
    let tags_body = body_json(tags.into_body()).await;
    assert_eq!(tags_body["beta"], "1.0.0");
    assert_eq!(tags_body["latest"], "1.0.0");
}

#[tokio::test]
async fn publish_supports_scoped_packages() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let bytes = b"scoped-tarball";
    let body = sample_publish_body("@scope/pkg", "1.0.0", bytes);
    let request = Request::put("/@scope/pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    assert!(storage.join("@scope/pkg/package.json").exists());
    assert!(storage.join("@scope/pkg/pkg-1.0.0.tgz").exists());

    // And we can read it back.
    let response =
        app.oneshot(Request::get("/@scope/pkg").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

/// `libnpmpublish` (the library `pnpm publish` / `npm publish` use under
/// the hood) names the `_attachments` key by the full package name —
/// for `@scope/name` that's `@scope/name-1.0.0.tgz`, with a literal `/`
/// in the filename. The server has to accept that shape and normalize
/// it to the canonical `<basename>-<version>.tgz` form on disk, otherwise
/// `pnpm publish` against pnpr fails with 400 for every scoped
/// package — see `recursivePublish.ts` in `@pnpm/releasing.commands`.
#[tokio::test]
async fn publish_accepts_libnpmpublish_scoped_attachment_filename() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let bytes = b"libnpmpublish-form";
    let pkg = "@pnpmtest/lib-pub-form";
    let version = "1.0.0";
    // _attachments key is the FULL scoped name + version, NOT the basename.
    let scoped_filename = format!("{pkg}-{version}.tgz");
    let body = json!({
        "_id": pkg,
        "name": pkg,
        "dist-tags": { "latest": version },
        "versions": {
            version: {
                "name": pkg,
                "version": version,
                "dist": {
                    "tarball": format!("http://localhost:4873/{pkg}/-/lib-pub-form-{version}.tgz"),
                    "shasum": sha1_hex(bytes),
                    "integrity": sri_sha512(bytes),
                },
            },
        },
        "_attachments": {
            scoped_filename: {
                "content_type": "application/octet-stream",
                "data": BASE64.encode(bytes),
                "length": bytes.len(),
            },
        },
    });
    let request = Request::put(format!("/{pkg}"))
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // On disk: canonical `<basename>-<version>.tgz` path, NOT the
    // scoped form. That's where serve_tarball looks.
    let on_disk = storage.join("@pnpmtest/lib-pub-form/lib-pub-form-1.0.0.tgz");
    assert!(on_disk.exists(), "tarball should be persisted at canonical path");
    assert_eq!(std::fs::read(&on_disk).unwrap(), bytes);

    // And it serves back via the spec URL form.
    let served = app
        .oneshot(
            Request::get("/@pnpmtest/lib-pub-form/-/lib-pub-form-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(served.status(), StatusCode::OK);
    let served_bytes = body_bytes(served.into_body()).await;
    assert_eq!(served_bytes, bytes);
}

#[tokio::test]
async fn search_finds_packages_by_substring_in_local_storage() {
    let storage = common::build_storage();
    let app = router(static_config(storage.path().to_path_buf()));
    let response = app
        .oneshot(Request::get("/-/v1/search?text=no-deps&size=20").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    let objects = body["objects"].as_array().expect("objects is array");
    assert!(!objects.is_empty(), "expected no-deps to match the storage fixture");
    let names: Vec<&str> =
        objects.iter().map(|object| object["package"]["name"].as_str().unwrap()).collect();
    assert!(names.iter().any(|n| n.contains("no-deps")), "got names: {names:?}");
}

#[tokio::test]
async fn search_filters_protected_packages_for_anonymous_callers() {
    let storage = common::build_storage();
    let app = router(static_config(storage.path().to_path_buf()));

    // Anonymous: `@pnpm.e2e/needs-auth` matches the access policy
    // for $authenticated, so search shouldn't surface it.
    let response = app
        .clone()
        .oneshot(Request::get("/-/v1/search?text=needs-auth&size=20").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    let names: Vec<&str> = body["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|object| object["package"]["name"].as_str().unwrap())
        .collect();
    assert!(
        !names.contains(&"@pnpm.e2e/needs-auth"),
        "anonymous search must not enumerate @pnpm.e2e/needs-auth; got {names:?}",
    );
    assert_eq!(body["total"], names.len(), "total must reflect post-filter count");

    // Authenticated: same query should return the package.
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;
    let response = app
        .oneshot(
            Request::get("/-/v1/search?text=needs-auth&size=20")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    let names: Vec<&str> = body["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|object| object["package"]["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"@pnpm.e2e/needs-auth"),
        "authenticated search must include @pnpm.e2e/needs-auth; got {names:?}",
    );
}

#[tokio::test]
async fn search_returns_empty_for_made_up_query() {
    let storage = common::build_storage();
    let app = router(static_config(storage.path().to_path_buf()));
    let response = app
        .oneshot(
            Request::get("/-/v1/search?text=zzz-does-not-exist-99999&size=20")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    assert_eq!(body["objects"].as_array().unwrap().len(), 0);
    assert_eq!(body["total"], 0);
}

#[tokio::test]
async fn search_augments_with_upstream_when_local_misses_exact_name() {
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::time::Duration;

    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({
        "name": "ghost-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "ghost-pkg",
                "version": "1.0.0",
                "description": "phantom dependency",
                "dist": { "tarball": format!("{}/ghost-pkg/-/ghost-pkg-1.0.0.tgz", upstream.url()) },
            },
        },
    });
    let _packument_mock = upstream
        .mock("GET", "/ghost-pkg")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
    let mut config = Config::proxy(listen, tmp.path().to_path_buf());
    config.uplinks.get_mut("npmjs").expect("default `npmjs` uplink").url = upstream.url();
    config.public_url = "http://example.test".to_string();
    config.packument_ttl = Duration::from_mins(1);
    let app = router(config);

    let response = app
        .oneshot(Request::get("/-/v1/search?text=ghost-pkg&size=20").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    let names: Vec<&str> = body["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|object| object["package"]["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"ghost-pkg"),
        "upstream-augment should surface exact-name match; got {names:?}",
    );
    assert_eq!(body["total"], names.len());

    // The augment also caches the packument in the disposable proxy
    // cache, so a subsequent search reuses it without another upstream
    // call.
    let on_disk = tmp.path().join(".pnpr-cache").join("ghost-pkg/package.json");
    assert!(on_disk.exists(), "augment must cache the fetched packument");
}

#[tokio::test]
async fn search_augment_skips_when_upstream_404s() {
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::time::Duration;

    let mut upstream = mockito::Server::new_async().await;
    let _mock = upstream
        .mock("GET", "/this-package-definitely-does-not-exist-xyz-123")
        .with_status(404)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
    let mut config = Config::proxy(listen, tmp.path().to_path_buf());
    config.uplinks.get_mut("npmjs").expect("default `npmjs` uplink").url = upstream.url();
    config.public_url = "http://example.test".to_string();
    config.packument_ttl = Duration::from_mins(1);
    let app = router(config);

    let response = app
        .oneshot(
            Request::get(
                "/-/v1/search?text=this-package-definitely-does-not-exist-xyz-123&size=20",
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    assert_eq!(body["objects"].as_array().unwrap().len(), 0);
    assert_eq!(body["total"], 0);
}

#[tokio::test]
async fn search_returns_empty_objects_in_static_mode() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let response = app
        .oneshot(Request::get("/-/v1/search?text=anything&size=20").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    assert_eq!(body["objects"].as_array().unwrap().len(), 0);
    assert_eq!(body["total"], 0);
}

#[tokio::test]
async fn unpublish_partial_writes_modified_packument() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    // Publish two versions, then PUT a modified packument with the
    // older one removed — simulating the partial-unpublish flow.
    for version in ["1.0.0", "2.0.0"] {
        let body = sample_publish_body("unpub-partial", version, version.as_bytes());
        let request = Request::put("/unpub-partial")
            .header("content-type", "application/json")
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        app.clone().oneshot(request).await.unwrap();
    }

    let modified = json!({
        "name": "unpub-partial",
        "_rev": "ignored",
        "dist-tags": { "latest": "2.0.0" },
        "versions": {
            "2.0.0": { "name": "unpub-partial", "version": "2.0.0", "dist": {
                "tarball": "http://example.test/unpub-partial/-/unpub-partial-2.0.0.tgz"
            }},
        },
    });
    let request = Request::put("/unpub-partial/-rev/anything")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&modified).unwrap()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // GET packument back — should only contain 2.0.0.
    let response = app
        .clone()
        .oneshot(Request::get("/unpub-partial").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let served = body_json(response.into_body()).await;
    assert_eq!(served["versions"].as_object().unwrap().keys().collect::<Vec<_>>(), vec!["2.0.0"]);

    // DELETE the 1.0.0 tarball next.
    let request = Request::delete("/unpub-partial/-/unpub-partial-1.0.0.tgz/-rev/anything")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(!storage.join("unpub-partial/unpub-partial-1.0.0.tgz").exists());
    assert!(storage.join("unpub-partial/unpub-partial-2.0.0.tgz").exists());

    // Second DELETE of the same tarball is a no-op — verdaccio
    // returns 201 here too (idempotent). The pnpm unpublish flow
    // tolerates 404 separately as a fallback, but we shouldn't even
    // get there.
    let request = Request::delete("/unpub-partial/-/unpub-partial-1.0.0.tgz/-rev/anything")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

/// Deleting a hosted tarball must also drop any proxied copy with the
/// same filename, so `open_tarball`'s cache fallback can't keep serving
/// the just-removed version.
#[tokio::test]
async fn unpublish_tarball_also_clears_the_proxied_copy() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let body = sample_publish_body("blend-pkg", "1.0.0", b"hosted-bytes");
    let request = Request::put("/blend-pkg")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);

    // Plant a stale proxied copy of the same tarball in the cache root,
    // as a `proxy:` rule would have left behind.
    let cached = storage.join(".pnpr-cache").join("blend-pkg");
    std::fs::create_dir_all(&cached).unwrap();
    std::fs::write(cached.join("blend-pkg-1.0.0.tgz"), b"stale-proxied-bytes").unwrap();

    let request = Request::delete("/blend-pkg/-/blend-pkg-1.0.0.tgz/-rev/anything")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);

    assert!(!storage.join("blend-pkg/blend-pkg-1.0.0.tgz").exists());
    assert!(!cached.join("blend-pkg-1.0.0.tgz").exists(), "proxied copy must be removed too");

    // With both stores cleared and no upstream, the version is gone.
    let response = app
        .oneshot(Request::get("/blend-pkg/-/blend-pkg-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unpublish_force_removes_entire_package() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let body = sample_publish_body("unpub-force", "1.0.0", b"contents");
    let request = Request::put("/unpub-force")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    app.clone().oneshot(request).await.unwrap();
    assert!(storage.join("unpub-force/package.json").exists());

    let request = Request::delete("/unpub-force/-rev/anything")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(!storage.join("unpub-force").exists());

    // Re-fetch returns 404 (static mode + no on-disk packument).
    let response =
        app.oneshot(Request::get("/unpub-force").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unpublish_scoped_tarball_via_six_segment_route() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let body = sample_publish_body("@scope/unpub", "1.0.0", b"bytes");
    let request = Request::put("/@scope/unpub")
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    app.clone().oneshot(request).await.unwrap();
    assert!(storage.join("@scope/unpub/unpub-1.0.0.tgz").exists());

    // pnpm reconstructs the DELETE URL from the rewritten tarball URL
    // in the packument, which uses literal `/` for the scope segment
    // (not `%2F`). That lands on the 6-seg route.
    let request = Request::delete("/@scope/unpub/-/unpub-1.0.0.tgz/-rev/anything")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(!storage.join("@scope/unpub/unpub-1.0.0.tgz").exists());
}

#[tokio::test]
async fn unpublish_requires_publish_auth() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let request = Request::delete("/some-pkg/-rev/anything").body(Body::empty()).unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// Two different versions of the same package, published concurrently,
/// both survive: the per-package serialization guard makes each publish
/// read-merge-write atomic, so neither overwrites the other's version
/// (the lost-update the guard exists to prevent). Without the guard the
/// two publishes can read the same empty packument and the second write
/// clobbers the first version.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_publishes_of_distinct_versions_all_survive() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;

    let publish = |version: &'static str| {
        let app = app.clone();
        let token = token.clone();
        tokio::spawn(async move {
            let body = sample_publish_body("racer", version, version.as_bytes());
            let request = Request::put("/racer")
                .header("content-type", "application/json")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap();
            app.oneshot(request).await.unwrap().status()
        })
    };

    let first_publish = publish("1.0.0");
    let second_publish = publish("2.0.0");
    let (first, second) = tokio::join!(first_publish, second_publish);
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(first, StatusCode::CREATED);
    assert_eq!(second, StatusCode::CREATED);

    let on_disk = std::fs::read(storage.join("racer/package.json")).expect("packument written");
    let packument: Value = serde_json::from_slice(&on_disk).unwrap();
    assert_eq!(packument["versions"]["1.0.0"]["version"], "1.0.0", "1.0.0 must survive");
    assert_eq!(packument["versions"]["2.0.0"]["version"], "2.0.0", "2.0.0 must survive");
}

fn sample_publish_body(name: &str, version: &str, tarball: &[u8]) -> Value {
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
