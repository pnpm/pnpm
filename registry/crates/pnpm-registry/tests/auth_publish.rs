//! Integration tests for the auth, dist-tag, and publish endpoints
//! added to support migrating `@pnpm/registry-mock` off verdaccio.
//! Static-mode (no upstream) to keep the tests hermetic.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::PathBuf;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

use pnpm_registry::{Config, router};

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

/// Use the registry-mock storage to verify that the auth policy
/// gates `@pnpm.e2e/needs-auth` correctly.
fn registry_mock_storage() -> Option<PathBuf> {
    std::env::current_dir().ok()?.ancestors().find_map(|dir| {
        let candidate = dir.join(
            "pacquet/tasks/registry-mock/node_modules/@pnpm/registry-mock/registry/storage-cache",
        );
        candidate.exists().then_some(candidate)
    })
}

#[tokio::test]
async fn anonymous_request_to_protected_package_returns_401() {
    let Some(storage) = registry_mock_storage() else {
        // Storage not populated — skip silently. `static_mode_returns_404_for_unknown_package`
        // in the sibling test file already covers static-mode plumbing without depending on
        // the fixtures, so we don't lose anything if the fixture is absent.
        return;
    };
    let app = router(static_config(storage));
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
    let Some(storage) = registry_mock_storage() else {
        return;
    };
    let app = router(static_config(storage));
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
    let Some(storage) = registry_mock_storage() else {
        return;
    };
    let app = router(static_config(storage));
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
/// `pnpm publish` against pnpm-registry fails with 400 for every scoped
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
                    "shasum": "deadbeef",
                    "integrity": "sha512-abcdef==",
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
    let Some(storage) = registry_mock_storage() else {
        return;
    };
    let app = router(static_config(storage));
    let response = app
        .oneshot(Request::get("/-/v1/search?text=is-positive&size=20").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    let objects = body["objects"].as_array().expect("objects is array");
    assert!(!objects.is_empty(), "expected is-positive to match the registry-mock fixture");
    let names: Vec<&str> =
        objects.iter().map(|object| object["package"]["name"].as_str().unwrap()).collect();
    assert!(names.iter().any(|n| n.contains("is-positive")), "got names: {names:?}");
}

#[tokio::test]
async fn search_returns_empty_for_made_up_query() {
    let Some(storage) = registry_mock_storage() else {
        return;
    };
    let app = router(static_config(storage));
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
                    "shasum": "deadbeef",
                    "integrity": "sha512-abcdef=="
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
