//! The OCI distribution surface: the version check, blob upload in one
//! request and in chunks, manifests, tags, the catalog, and the access rules
//! every one of them goes through.

#[path = "oci_registry/referrers.rs"]
mod referrers;

#[path = "oci_registry/authorization.rs"]
mod authorization;

#[path = "oci_registry/discovery.rs"]
mod discovery;

#[path = "oci_registry/manifests.rs"]
mod manifests;

#[path = "oci_registry/uploads.rs"]
mod uploads;

#[path = "oci_registry/behavior.rs"]
mod behavior;

// `#[path]` rather than the `tests/common/mod.rs` layout, which the
// Perfectionist dylint forbids.
#[path = "common/ecosystem.rs"]
#[allow(
    dead_code,
    reason = "the shared fixtures include helpers for the Cargo and Python surfaces"
)]
mod common;

#[path = "common/pausing_store.rs"]
mod pausing_store;

#[path = "../src/server/striped_locks.rs"]
#[allow(dead_code, reason = "the collision fixture uses the production stripe mapping")]
mod striped_locks;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use common::{HostedSource, body_bytes, mixed_router_config, sha256_hex};
use pnpr::{
    AccessList, AuthState, Config, Ecosystem, PackagePattern, PackageRule, PackageRules,
    router_with_auth,
};
use serde_json::{Value, json};
use std::path::PathBuf;
use tempfile::TempDir;
use tower::ServiceExt;

/// The challenge a 401 carries, which is what tells a client to retry with
/// credentials.
const CHALLENGE: &str = r#"Basic realm="pnpr""#;

/// A hosted image registry (`images`, claiming the `acme` namespace) beside
/// an image upstream that claims everything else.
fn oci_config(storage: PathBuf, hosted_access: &str) -> Config {
    mixed_router_config(
        storage,
        Ecosystem::Oci,
        HostedSource {
            name: "images",
            org: "images",
            access: hosted_access,
            packages: &["acme/*"],
        },
        ("dockerhub", "http://images.invalid/"),
    )
}

fn app(tmp: &TempDir) -> Router {
    router_with_auth(oci_config(tmp.path().to_path_buf(), "$all"), AuthState::in_memory())
}

/// The same registry with destructive writes opened up, which the
/// registry-level default denies.
fn app_allowing_deletes(tmp: &TempDir) -> Router {
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    let hosted = config.hosted.get_mut("images").expect("the hosted image registry");
    hosted.rules = std::mem::take(&mut hosted.rules)
        .with_default_unpublish(AccessList::from_tokens(["$authenticated"]));
    router_with_auth(config, AuthState::in_memory())
}

fn digest_of(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(bytes))
}

async fn token(app: &Router) -> String {
    let body = json!({
        "_id": "org.couchdb.user:alice",
        "name": "alice",
        "password": "secret",
        "email": "alice@example.com",
        "type": "user",
        "roles": [],
    });
    let request = Request::put("/-/user/org.couchdb.user:alice")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    payload["token"].as_str().expect("token in response").to_string()
}

/// `docker login` sends the token as the `Basic` password.
fn basic(token: &str) -> String {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    format!("Basic {}", STANDARD.encode(format!("alice:{token}")))
}

/// Push `bytes` as a blob of `repository` in one request, returning its digest.
async fn push_blob(app: &Router, auth: &str, repository: &str, bytes: &[u8]) -> String {
    let digest = digest_of(bytes);
    let request = Request::post(format!("/v2/{repository}/blobs/uploads/?digest={digest}"))
        .header(header::AUTHORIZATION, auth)
        .body(Body::from(bytes.to_vec()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED, "blob push should succeed");
    digest
}

fn image_manifest(config: &str, layers: &[&str]) -> Vec<u8> {
    let layer_values: Vec<Value> = layers
        .iter()
        .map(|layer| json!({ "digest": digest_of(layer.as_bytes()), "size": layer.len() }))
        .collect();
    serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": { "digest": digest_of(config.as_bytes()), "size": config.len() },
        "layers": layer_values,
    }))
    .unwrap()
}

/// Push a complete image and return the manifest's digest.
async fn push_image(app: &Router, auth: &str, repository: &str, reference: &str) -> String {
    push_blob(app, auth, repository, b"config").await;
    push_blob(app, auth, repository, b"layer").await;
    let manifest = image_manifest("config", &["layer"]);
    let request = Request::put(format!("/v2/{repository}/manifests/{reference}"))
        .header(header::AUTHORIZATION, auth)
        .header(header::CONTENT_TYPE, "application/vnd.oci.image.manifest.v1+json")
        .body(Body::from(manifest.clone()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED, "manifest push should succeed");
    digest_of(&manifest)
}

async fn get(app: &Router, path: &str) -> axum::response::Response {
    app.clone().oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap()
}

async fn check_protocol_surface(app: Router) {
    let auth = basic(&token(&app).await);
    let digest = push_blob(&app, &auth, "acme/source", b"0123456789").await;
    let blob_path = format!("/v2/acme/source/blobs/{digest}");
    check_satisfiable_ranges(&app, &blob_path).await;
    check_unsatisfiable_ranges(&app, &blob_path).await;
    check_ignored_ranges(&app, &blob_path).await;
    check_if_range(&app, &blob_path, &digest).await;
    let response = app
        .clone()
        .oneshot(
            Request::head(&blob_path)
                .header(header::RANGE, "bytes=7-")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_LENGTH], "10");
    assert!(body_bytes(response.into_body()).await.is_empty());
    let empty = push_blob(&app, &auth, "acme/source", b"").await;
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/v2/acme/source/blobs/{empty}"))
                .header(header::RANGE, "bytes=0-")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes */0");

    let response = app
        .clone()
        .oneshot(
            Request::post(format!(
                "/v2/acme/destination/blobs/uploads/?mount={digest}&from=acme%2Fsource",
            ))
            .header(header::AUTHORIZATION, &auth)
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let location = response.headers()[header::LOCATION].to_str().unwrap();
    assert_eq!(location, format!("/v2/acme/destination/blobs/{digest}"));
    assert_eq!(body_bytes(get(&app, location).await.into_body()).await, b"0123456789");
    for from in ["acme/missing", "library/upstream"] {
        let response = app
            .clone()
            .oneshot(
                Request::post(format!(
                    "/v2/acme/destination/blobs/uploads/?mount={digest}&from={from}",
                ))
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert!(response.headers()[header::LOCATION].to_str().unwrap().contains("/uploads/"));
    }

    for tag in ["a", "c", "e"] {
        push_image(&app, &auth, "acme/pages", tag).await;
    }
    push_image(&app, &auth, "acme/zebra", "latest").await;
    let response = get(&app, "/v2/acme/pages/tags/list?n=2").await;
    let link = response.headers()[header::LINK].to_str().unwrap().to_string();
    assert_eq!(link, r#"</v2/acme/pages/tags/list?n=2&last=c>; rel="next""#);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["tags"], json!(["a", "c"]));
    let response = get(&app, "/v2/acme/pages/tags/list?n=2&last=c").await;
    assert!(!response.headers().contains_key(header::LINK));
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["tags"], json!(["e"]));
    let response = get(&app, "/v2/acme/pages/tags/list?last=b").await;
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["tags"], json!(["c", "e"]));
    for endpoint in ["acme/pages/tags/list", "_catalog"] {
        let response = get(&app, &format!("/v2/{endpoint}?n=0")).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key(header::LINK));
        let payload: Value =
            serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        let field = if endpoint == "_catalog" { "repositories" } else { "tags" };
        assert_eq!(payload[field], json!([]));
        for invalid in ["-1", "oops", "184467440737095516160", ""] {
            assert_eq!(
                get(&app, &format!("/v2/{endpoint}?n={invalid}")).await.status(),
                StatusCode::BAD_REQUEST,
            );
        }
    }
    let response = get(&app, "/oci/~images/v2/_catalog?n=1").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()[header::LINK]
            .to_str()
            .unwrap()
            .starts_with("</oci/~images/v2/_catalog?"),
    );
    let response = get(&app, "/v2/_catalog?n=1&last=acme%2Fpages").await;
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["repositories"], json!(["acme/zebra"]));

    let subject = digest_of(b"not pushed yet");
    let path = format!("/v2/acme/artifacts/referrers/{subject}");
    let response = get(&app, &path).await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["manifests"], json!([]));
    let config = push_blob(&app, &auth, "acme/artifacts", b"{}").await;
    let manifest = json!({ "schemaVersion": 2, "mediaType": pnpr_oci::media_type::OCI_IMAGE_MANIFEST,
        "config": { "digest": config, "size": 2, "mediaType": "application/example.signature" },
        "layers": [], "subject": { "digest": subject, "size": 42 },
        "annotations": { "example.key": "value" } });
    let bytes = serde_json::to_vec(&manifest).unwrap();
    let referrer = digest_of(&bytes);
    for tag in ["signature", "another-tag"] {
        let response = app
            .clone()
            .oneshot(
                Request::put(format!("/v2/acme/artifacts/manifests/{tag}"))
                    .header(header::AUTHORIZATION, &auth)
                    .body(Body::from(bytes.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()["oci-subject"], subject);
    }
    let response = get(&app, &format!("{path}?artifactType=application%2Fexample.signature")).await;
    assert_eq!(response.headers()[header::CONTENT_TYPE], pnpr_oci::media_type::OCI_IMAGE_INDEX);
    assert_eq!(response.headers()["oci-filters-applied"], "artifactType");
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["schemaVersion"], 2);
    assert_eq!(
        payload["manifests"],
        json!([{ "mediaType": pnpr_oci::media_type::OCI_IMAGE_MANIFEST,
        "digest": referrer, "size": bytes.len(), "artifactType": "application/example.signature",
        "annotations": { "example.key": "value" } }]),
    );
    let response = get(&app, &format!("{path}?artifactType=other")).await;
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["manifests"], json!([]));
    let response = app
        .clone()
        .oneshot(
            Request::delete("/v2/acme/artifacts/manifests/signature")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let response = get(&app, &path).await;
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["manifests"].as_array().unwrap().len(), 1);
    let response = app
        .clone()
        .oneshot(
            Request::delete(format!("/v2/acme/artifacts/manifests/{referrer}"))
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let response = get(&app, &path).await;
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["manifests"], json!([]));
    assert_eq!(
        get(&app, "/v2/acme/artifacts/referrers/invalid").await.status(),
        StatusCode::BAD_REQUEST,
    );
}

/// A range is honoured only while the caller's validator still matches.
async fn check_if_range(app: &Router, blob_path: &str, digest: &str) {
    for (validator, status) in [
        (format!(r#""{digest}""#), StatusCode::PARTIAL_CONTENT),
        (r#""other""#.into(), StatusCode::OK),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(blob_path)
                    .header(header::RANGE, "bytes=7-")
                    .header(header::IF_RANGE, validator)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
}

/// A range this server does not implement serves the whole blob.
async fn check_ignored_ranges(app: &Router, blob_path: &str) {
    for range in ["items=1-2", "bytes=1-2,4-5", "bytes=9-1", "bytes=+1-2"] {
        let response = app
            .clone()
            .oneshot(
                Request::get(blob_path).header(header::RANGE, range).body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{range}");
        assert_eq!(body_bytes(response.into_body()).await, b"0123456789");
    }
}

/// A range outside the blob is refused with `416`.
async fn check_unsatisfiable_ranges(app: &Router, blob_path: &str) {
    for range in ["bytes=10-", "bytes=-0", "bytes=99-100"] {
        let response = app
            .clone()
            .oneshot(
                Request::get(blob_path).header(header::RANGE, range).body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE, "{range}");
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes */10");
    }
}

/// A range the blob can satisfy is served as `206` with the bytes it names.
async fn check_satisfiable_ranges(app: &Router, blob_path: &str) {
    for (range, expected, content_range) in [
        ("bytes=2-5", "2345", "bytes 2-5/10"),
        ("bytes=7-", "789", "bytes 7-9/10"),
        ("bytes=-3", "789", "bytes 7-9/10"),
        ("bytes=7-999", "789", "bytes 7-9/10"),
        ("bytes=-99", "0123456789", "bytes 0-9/10"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(blob_path).header(header::RANGE, range).body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT, "{range}");
        assert_eq!(response.headers()[header::CONTENT_RANGE], content_range);
        assert_eq!(response.headers()[header::CONTENT_LENGTH], expected.len().to_string());
        assert_eq!(response.headers()[header::ACCEPT_RANGES], "bytes");
        assert_eq!(body_bytes(response.into_body()).await, expected.as_bytes());
    }
}

fn strip_referrer_metadata(document: &mut Value, expected_count: usize) {
    let entries = document["manifests"].as_array_mut().unwrap();
    assert_eq!(entries.len(), expected_count, "the pushed manifests are stored");
    for entry in entries {
        assert!(
            entry.as_object_mut().unwrap().remove("referrer").is_some(),
            "the stored entry must carry referrer metadata before stripping it",
        );
    }
}

fn repository_with_colliding_lock_keys() -> String {
    let locks = striped_locks::StripedLocks::new();
    (0..4096)
        .map(|index| format!("acme/lock-collision-{index}"))
        .find(|name| {
            locks.stripe_index(name) == locks.stripe_index(&format!("oci-referrers:{name}"))
        })
        .expect("a repository whose lock keys collide")
}
