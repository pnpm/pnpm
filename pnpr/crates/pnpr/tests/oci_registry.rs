//! The OCI distribution surface: the version check, blob upload in one
//! request and in chunks, manifests, tags, the catalog, and the access rules
//! every one of them goes through.

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

#[tokio::test]
async fn the_version_check_answers_at_the_host_root() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    let request =
        Request::get("/v2/").header(header::AUTHORIZATION, &auth).body(Body::empty()).unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("docker-distribution-api-version").unwrap(), "registry/2.0");
}

#[tokio::test]
async fn the_version_check_challenges_an_anonymous_caller_even_where_reads_are_open() {
    let tmp = TempDir::new().unwrap();
    // Reads are `$all` here, and the challenge still has to come: a client
    // settles its authentication scheme on this one response, so a 200 would
    // leave it with no way to authenticate a later push.
    let response = get(&app(&tmp), "/v2/").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers().get(header::WWW_AUTHENTICATE).unwrap(), CHALLENGE);
}

#[tokio::test]
async fn a_challenged_ping_does_not_stop_an_anonymous_pull() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;

    assert_eq!(get(&app, "/v2/").await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(get(&app, "/v2/acme/app/manifests/1.0").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn an_image_pushed_in_one_request_each_pulls_back() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    let manifest_digest = push_image(&app, &auth, "acme/app", "1.0").await;

    let response = get(&app, "/v2/acme/app/manifests/1.0").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("docker-content-digest").unwrap(), &manifest_digest);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/vnd.oci.image.manifest.v1+json",
    );
    assert_eq!(body_bytes(response.into_body()).await, image_manifest("config", &["layer"]));

    let response = get(&app, &format!("/v2/acme/app/manifests/{manifest_digest}")).await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = get(&app, &format!("/v2/acme/app/blobs/{}", digest_of(b"layer"))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, b"layer".as_slice());
}

#[tokio::test]
async fn a_head_request_carries_the_headers_without_the_body() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;

    for path in
        ["/v2/acme/app/manifests/1.0", &format!("/v2/acme/app/blobs/{}", digest_of(b"layer"))]
    {
        let request = Request::head(path).body(Body::empty()).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert!(response.headers().contains_key("docker-content-digest"), "{path}");
        assert!(response.headers().contains_key(header::CONTENT_LENGTH), "{path}");
        assert!(body_bytes(response.into_body()).await.is_empty(), "{path}");
    }
}

#[tokio::test]
async fn a_chunked_upload_resumes_from_where_it_left_off() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    let request = Request::post("/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap().to_string();
    assert_eq!(response.headers().get(header::RANGE).unwrap(), "0-0");

    let request = Request::patch(&location)
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_RANGE, "0-4")
        .body(Body::from("hello"))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(response.headers().get(header::RANGE).unwrap(), "0-4");

    let request = Request::patch(&location)
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_RANGE, "99-103")
        .body(Body::from("nope!"))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);

    let request = Request::patch(&location)
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_RANGE, "5-10")
        .body(Body::from(" world"))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let digest = digest_of(b"hello world");
    let request = Request::put(format!("{location}?digest={digest}"))
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let response = get(&app, &format!("/v2/acme/app/blobs/{digest}")).await;
    assert_eq!(body_bytes(response.into_body()).await, b"hello world".as_slice());
}

#[tokio::test]
async fn bytes_that_do_not_match_the_promised_digest_are_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    let lie = digest_of(b"something else");
    let request = Request::post(format!("/v2/acme/app/blobs/uploads/?digest={lie}"))
        .header(header::AUTHORIZATION, &auth)
        .body(Body::from("real bytes"))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = get(&app, &format!("/v2/acme/app/blobs/{lie}")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_manifest_naming_a_blob_the_repository_lacks_is_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_blob(&app, &auth, "acme/app", b"config").await;

    let request = Request::put("/v2/acme/app/manifests/1.0")
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_TYPE, "application/vnd.oci.image.manifest.v1+json")
        .body(Body::from(image_manifest("config", &["layer"])))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["errors"][0]["code"], "MANIFEST_BLOB_UNKNOWN");

    let response = get(&app, "/v2/acme/app/manifests/1.0").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_manifest_pushed_under_the_wrong_digest_is_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_blob(&app, &auth, "acme/app", b"config").await;
    push_blob(&app, &auth, "acme/app", b"layer").await;

    let wrong = digest_of(b"not the manifest");
    let request = Request::put(format!("/v2/acme/app/manifests/{wrong}"))
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_TYPE, "application/vnd.oci.image.manifest.v1+json")
        .body(Body::from(image_manifest("config", &["layer"])))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["errors"][0]["code"], "DIGEST_INVALID");
}

#[tokio::test]
async fn tags_list_in_lexical_order_and_a_moved_tag_repoints() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;
    push_image(&app, &auth, "acme/app", "latest").await;

    let response = get(&app, "/v2/acme/app/tags/list").await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["name"], "acme/app");
    assert_eq!(payload["tags"], json!(["1.0", "latest"]));

    push_blob(&app, &auth, "acme/app", b"config2").await;
    let moved = image_manifest("config2", &[]);
    let request = Request::put("/v2/acme/app/manifests/latest")
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_TYPE, "application/vnd.oci.image.manifest.v1+json")
        .body(Body::from(moved.clone()))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);

    let response = get(&app, "/v2/acme/app/manifests/latest").await;
    assert_eq!(response.headers().get("docker-content-digest").unwrap(), &digest_of(&moved));
}

#[tokio::test]
async fn deleting_a_tag_keeps_the_manifest_and_deleting_the_manifest_drops_the_tag() {
    let tmp = TempDir::new().unwrap();
    let app = app_allowing_deletes(&tmp);
    let auth = basic(&token(&app).await);
    let manifest_digest = push_image(&app, &auth, "acme/app", "1.0").await;

    let request = Request::delete("/v2/acme/app/manifests/1.0")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::ACCEPTED);
    assert_eq!(get(&app, "/v2/acme/app/manifests/1.0").await.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        get(&app, &format!("/v2/acme/app/manifests/{manifest_digest}")).await.status(),
        StatusCode::OK,
    );

    let request = Request::delete(format!("/v2/acme/app/manifests/{manifest_digest}"))
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::ACCEPTED);
    assert_eq!(
        get(&app, &format!("/v2/acme/app/manifests/{manifest_digest}")).await.status(),
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn the_catalog_lists_only_hosted_repositories() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;
    push_image(&app, &auth, "acme/team/tool", "1.0").await;

    let response = get(&app, "/v2/_catalog").await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["repositories"], json!(["acme/app", "acme/team/tool"]));
}

#[tokio::test]
async fn an_anonymous_push_is_refused_with_a_challenge() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);

    let request = Request::post("/v2/acme/app/blobs/uploads/").body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers().get(header::WWW_AUTHENTICATE).unwrap(), CHALLENGE);
}

#[tokio::test]
async fn a_name_no_hosted_registry_claims_is_not_served() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    // `other/app` routes to the image upstream, which is not proxied yet.
    let request = Request::post("/v2/other/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    assert_eq!(get(&app, "/v2/other/app/tags/list").await.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_repository_name_the_grammar_refuses_never_reaches_storage() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);

    for name in ["acme/../etc", "acme/.hidden", "acme/-leading"] {
        let response = get(&app, &format!("/v2/{name}/tags/list")).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{name}");
        let payload: Value =
            serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        assert_eq!(payload["errors"][0]["code"], "NAME_INVALID", "{name}");
    }
}

#[tokio::test]
async fn a_repository_name_is_case_folded_rather_than_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;

    // Two spellings are one repository, not two directories that a
    // case-insensitive filesystem would then collide.
    let response = get(&app, "/v2/ACME/App/tags/list").await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["name"], "acme/app");
    assert_eq!(payload["tags"], json!(["1.0"]));
}

#[tokio::test]
async fn the_catalog_omits_repositories_the_caller_may_not_read() {
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    let hosted = config.hosted.get_mut("images").expect("the hosted image registry");
    // Reads are open by default, and `acme/secret` refines that to require a
    // caller. A listing must apply the same rule its fetches would.
    hosted.rules = PackageRules::new(
        vec![PackageRule {
            pattern: PackagePattern::parse("acme/secret", Ecosystem::Oci).unwrap(),
            access: Some(AccessList::from_tokens(["$authenticated"])),
            publish: None,
            unpublish: None,
        }],
        Some(AccessList::from_tokens(["$all"])),
    );
    let app = router_with_auth(config, AuthState::in_memory());
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;
    push_image(&app, &auth, "acme/secret", "1.0").await;

    let response = get(&app, "/v2/_catalog").await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["repositories"], json!(["acme/app"]));

    let request = Request::get("/v2/_catalog")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["repositories"], json!(["acme/app", "acme/secret"]));
}

#[tokio::test]
async fn the_named_form_serves_the_same_repository() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;

    let response = get(&app, "/oci/~images/v2/acme/app/tags/list").await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["tags"], json!(["1.0"]));
}

#[tokio::test]
async fn an_upload_started_through_a_prefixed_base_continues_there() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    let request = Request::post("/oci/~images/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap();
    assert!(
        location.starts_with("/oci/~images/v2/acme/app/blobs/uploads/"),
        "an upload must continue where it started, got {location}",
    );
}

#[tokio::test]
async fn a_delete_is_refused_unless_the_registry_opens_destructive_writes() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;

    let request = Request::delete("/v2/acme/app/manifests/1.0")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(get(&app, "/v2/acme/app/manifests/1.0").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_manifest_whose_descriptor_size_is_wrong_is_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_blob(&app, &auth, "acme/app", b"config").await;
    push_blob(&app, &auth, "acme/app", b"layer").await;

    // The blobs are there, but the manifest lies about how long the layer is,
    // which a client that verifies descriptors would refuse to pull.
    let manifest = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": { "digest": digest_of(b"config"), "size": 6 },
        "layers": [{ "digest": digest_of(b"layer"), "size": 9999 }],
    }))
    .unwrap();
    let request = Request::put("/v2/acme/app/manifests/1.0")
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_TYPE, "application/vnd.oci.image.manifest.v1+json")
        .body(Body::from(manifest))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["errors"][0]["code"], "MANIFEST_INVALID");
}

#[tokio::test]
async fn a_reference_that_is_neither_tag_nor_digest_is_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;

    // `sha256:short` parses as neither, and storing it verbatim would leave a
    // digest-shaped entry in the tag list.
    for reference in ["sha256:short", ".leading-dot", "-leading-dash"] {
        let request = Request::put(format!("/v2/acme/app/manifests/{reference}"))
            .header(header::AUTHORIZATION, &auth)
            .header(header::CONTENT_TYPE, "application/vnd.oci.image.manifest.v1+json")
            .body(Body::from(image_manifest("config", &["layer"])))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{reference} should not be a tag");
    }

    let response = get(&app, "/v2/acme/app/tags/list").await;
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["tags"], json!(["1.0"]));
}

#[tokio::test]
async fn reads_of_a_private_repository_are_kept_out_of_shared_caches() {
    let tmp = TempDir::new().unwrap();
    let config = oci_config(tmp.path().to_path_buf(), "$authenticated");
    let app = router_with_auth(config, AuthState::in_memory());
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;

    for path in ["/v2/acme/app/manifests/1.0", "/v2/acme/app/tags/list", "/v2/_catalog"] {
        let request =
            Request::get(path).header(header::AUTHORIZATION, &auth).body(Body::empty()).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).map(|value| value.to_str().unwrap()),
            Some("private, no-store"),
            "{path} must not be storable by a shared cache",
        );
    }
}

#[tokio::test]
async fn concurrent_chunks_of_one_upload_neither_lose_nor_duplicate_bytes() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    let request = Request::post("/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap().to_string();

    // Requests for one upload are serialized, so whichever order these land in
    // the upload holds exactly both chunks. Without that the two appends could
    // interleave, and the digest a later PUT verified would not be the bytes
    // that got promoted.
    let patch = |chunk: &'static str| {
        let app = app.clone();
        let auth = auth.clone();
        let location = location.clone();
        async move {
            let request = Request::patch(&location)
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from(chunk))
                .unwrap();
            app.oneshot(request).await.unwrap().status()
        }
    };
    let one = patch("aaaaa");
    let two = patch("bbbbb");
    let (first, second) = tokio::join!(one, two);
    assert_eq!(first, StatusCode::ACCEPTED);
    assert_eq!(second, StatusCode::ACCEPTED);

    let request =
        Request::get(&location).header(header::AUTHORIZATION, &auth).body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.headers().get(header::RANGE).unwrap(), "0-9");
}

#[tokio::test]
async fn a_manifest_repeating_one_descriptor_is_not_thousands_of_lookups() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_blob(&app, &auth, "acme/app", b"config").await;
    push_blob(&app, &auth, "acme/app", b"layer").await;

    // One digest repeated far past any real image. It is looked up once, so
    // this is accepted rather than turned into a lookup per entry.
    let layer = json!({ "digest": digest_of(b"layer"), "size": 5 });
    let manifest = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": { "digest": digest_of(b"config"), "size": 6 },
        "layers": vec![layer; 20_000],
    }))
    .unwrap();
    let request = Request::put("/v2/acme/app/manifests/1.0")
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_TYPE, "application/vnd.oci.image.manifest.v1+json")
        .body(Body::from(manifest))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn an_index_child_must_be_a_manifest_this_repository_serves() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    // A layer blob is present, but it is not a manifest. An index naming it
    // would publish a child that a client asking for it cannot pull.
    push_blob(&app, &auth, "acme/app", b"layer").await;

    let index = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [{ "digest": digest_of(b"layer"), "size": 5 }],
    }))
    .unwrap();
    let request = Request::put("/v2/acme/app/manifests/multi")
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_TYPE, "application/vnd.oci.image.index.v1+json")
        .body(Body::from(index))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(get(&app, "/v2/acme/app/manifests/multi").await.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_index_over_pushed_children_publishes() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    // The child is pushed first, the way a multi-architecture push does it,
    // so the index that names it resolves.
    let child = push_image(&app, &auth, "acme/app", "child").await;

    let index = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [{ "digest": child, "size": image_manifest("config", &["layer"]).len() }],
    }))
    .unwrap();
    let request = Request::put("/v2/acme/app/manifests/multi")
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_TYPE, "application/vnd.oci.image.index.v1+json")
        .body(Body::from(index))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);
    assert_eq!(get(&app, "/v2/acme/app/manifests/multi").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn deleting_a_referenced_blob_is_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app_allowing_deletes(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;

    let request = Request::delete(format!("/v2/acme/app/blobs/{}", digest_of(b"layer")))
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        get(&app, &format!("/v2/acme/app/blobs/{}", digest_of(b"layer"))).await.status(),
        StatusCode::OK,
    );
}

#[tokio::test]
async fn a_malformed_content_range_does_not_advance_an_upload() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    let request = Request::post("/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap().to_string();

    // The leading number is where the upload actually stands, so a range read
    // only up to the hyphen would accept this and write the body.
    let request = Request::patch(&location)
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_RANGE, "0-garbage")
        .body(Body::from("nope!"))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let request =
        Request::get(&location).header(header::AUTHORIZATION, &auth).body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.headers().get(header::RANGE).unwrap(), "0-0");
}

#[tokio::test]
async fn a_range_that_contradicts_itself_or_the_body_is_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    let request = Request::post("/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap().to_string();

    // An end before the start, and a body that is not the length the range
    // declares. Both would otherwise leave the upload somewhere neither side
    // named.
    for (range, body) in [("5-2", "hello"), ("0-99", "hello")] {
        let request = Request::patch(&location)
            .header(header::AUTHORIZATION, &auth)
            .header(header::CONTENT_RANGE, range)
            .header(header::CONTENT_LENGTH, body.len())
            .body(Body::from(body))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{range}");
    }

    let request =
        Request::get(&location).header(header::AUTHORIZATION, &auth).body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.headers().get(header::RANGE).unwrap(), "0-0");
}

#[tokio::test]
async fn a_range_spanning_more_than_a_blob_can_hold_is_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    let request = Request::post("/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap().to_string();

    // One past the largest range a client can spell does not fit the number
    // that holds the span, and no chunk may be larger than a whole blob.
    for range in ["0-18446744073709551615", "0-999999999999"] {
        let request = Request::patch(&location)
            .header(header::AUTHORIZATION, &auth)
            .header(header::CONTENT_RANGE, range)
            .body(Body::from("hello"))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{range}");
    }

    let request =
        Request::get(&location).header(header::AUTHORIZATION, &auth).body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.headers().get(header::RANGE).unwrap(), "0-0");
}

#[tokio::test]
async fn a_path_that_names_no_endpoint_is_not_found() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);

    // A slash inside a reference decodes into another path segment, which
    // leaves a tail naming no endpoint rather than a manifest with an odd
    // name. Nothing is served there, which is not the same as a method being
    // refused on something that is.
    for path in ["/v2/acme/app/manifests/has%2Fslash", "/v2/acme/app/nonsense/1.0", "/v2/acme"] {
        let response = get(&app, path).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn an_upload_cannot_be_finished_into_another_repository() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    let request = Request::post("/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap().to_string();
    let id = location.rsplit('/').next().unwrap().to_string();

    // The same organization, and the caller may publish to both. The id still
    // only names bytes the other repository's client sent.
    let request = Request::patch(format!("/v2/acme/other/blobs/uploads/{id}"))
        .header(header::AUTHORIZATION, &auth)
        .body(Body::from("hello"))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_chunk_that_ends_early_leaves_the_prefix_to_resume_from() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    let request = Request::post("/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap().to_string();

    let torn = futures_util::stream::iter([
        Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"hello")),
        Err(std::io::Error::other("the connection went away")),
    ]);
    let request = Request::patch(&location)
        .header(header::AUTHORIZATION, &auth)
        .body(Body::from_stream(torn))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // The bytes that did arrive are an ordered prefix of the blob, so the
    // upload keeps them and says so. Dropping them would cost the client the
    // whole layer for one lost connection.
    let request =
        Request::get(&location).header(header::AUTHORIZATION, &auth).body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.headers().get(header::RANGE).unwrap(), "0-4");

    let request = Request::patch(&location)
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_RANGE, "5-10")
        .body(Body::from(" world"))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let digest = digest_of(b"hello world");
    let request = Request::put(format!("{location}?digest={digest}"))
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn s3_upload_moves_between_replicas_and_keeps_an_interrupted_chunks_prefix() {
    let first_disk = TempDir::new().unwrap();
    let second_disk = TempDir::new().unwrap();
    let objects: std::sync::Arc<dyn object_store::ObjectStore> =
        std::sync::Arc::new(object_store::memory::InMemory::new());
    let auth_state = AuthState::in_memory();
    let replica = |disk: &TempDir| {
        let mut config = oci_config(disk.path().to_path_buf(), "$all");
        config.hosted_store = pnpr::HostedStoreConfig::ObjectStore {
            store: std::sync::Arc::clone(&objects),
            prefix: "shared/".into(),
        };
        router_with_auth(config, auth_state.clone())
    };
    let first = replica(&first_disk);
    let second = replica(&second_disk);
    let auth = basic(&token(&first).await);
    let response = first
        .clone()
        .oneshot(
            Request::post("/v2/acme/app/blobs/uploads/")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from("hello "))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let location = response.headers()[header::LOCATION].to_str().unwrap().to_string();
    let torn = futures_util::stream::iter([
        Ok(axum::body::Bytes::from_static(b"wor")),
        Err(std::io::Error::other("disconnected")),
    ]);
    let response = second
        .clone()
        .oneshot(
            Request::patch(&location)
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from_stream(torn))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = first
        .clone()
        .oneshot(
            Request::get(&location)
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()[header::RANGE], "0-8");
    let digest = digest_of(b"hello world");
    let response = first
        .clone()
        .oneshot(
            Request::put(format!("{location}?digest={digest}"))
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from("ld"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let response = get(&second, &format!("/v2/acme/app/blobs/{digest}")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, b"hello world");
}

#[tokio::test]
async fn protocol_surface_on_filesystem() {
    let tmp = TempDir::new().unwrap();
    check_protocol_surface(app_allowing_deletes(&tmp)).await;
}

#[tokio::test]
async fn protocol_surface_on_object_store() {
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.hosted_store = pnpr::HostedStoreConfig::ObjectStore {
        store: std::sync::Arc::new(object_store::memory::InMemory::new()),
        prefix: "protocol/".into(),
    };
    let hosted = config.hosted.get_mut("images").unwrap();
    hosted.rules = std::mem::take(&mut hosted.rules)
        .with_default_unpublish(AccessList::from_tokens(["$authenticated"]));
    check_protocol_surface(router_with_auth(config, AuthState::in_memory())).await;
}

async fn check_protocol_surface(app: Router) {
    let auth = basic(&token(&app).await);
    let digest = push_blob(&app, &auth, "acme/source", b"0123456789").await;
    let blob_path = format!("/v2/acme/source/blobs/{digest}");
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
                Request::get(&blob_path).header(header::RANGE, range).body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT, "{range}");
        assert_eq!(response.headers()[header::CONTENT_RANGE], content_range);
        assert_eq!(response.headers()[header::CONTENT_LENGTH], expected.len().to_string());
        assert_eq!(response.headers()[header::ACCEPT_RANGES], "bytes");
        assert_eq!(body_bytes(response.into_body()).await, expected.as_bytes());
    }
    for range in ["bytes=10-", "bytes=-0", "bytes=99-100"] {
        let response = app
            .clone()
            .oneshot(
                Request::get(&blob_path).header(header::RANGE, range).body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE, "{range}");
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes */10");
    }
    for range in ["items=1-2", "bytes=1-2,4-5", "bytes=9-1", "bytes=+1-2"] {
        let response = app
            .clone()
            .oneshot(
                Request::get(&blob_path).header(header::RANGE, range).body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{range}");
        assert_eq!(body_bytes(response.into_body()).await, b"0123456789");
    }
    for (validator, status) in [
        (format!(r#""{digest}""#), StatusCode::PARTIAL_CONTENT),
        (r#""other""#.into(), StatusCode::OK),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(&blob_path)
                    .header(header::RANGE, "bytes=7-")
                    .header(header::IF_RANGE, validator)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
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

#[tokio::test]
async fn mounts_and_discovery_respect_source_read_permissions() {
    let tmp = TempDir::new().unwrap();
    let auth_state = AuthState::in_memory();
    let setup = router_with_auth(oci_config(tmp.path().to_path_buf(), "$all"), auth_state.clone());
    let auth = basic(&token(&setup).await);
    let digest = push_blob(&setup, &auth, "acme/secret", b"secret layer").await;
    push_image(&setup, &auth, "acme/secret", "latest").await;
    push_image(&setup, &auth, "acme/public", "latest").await;
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.hosted.get_mut("images").unwrap().rules = PackageRules::new(
        vec![PackageRule {
            pattern: PackagePattern::parse("acme/secret", Ecosystem::Oci).unwrap(),
            access: Some(AccessList::from_tokens(["bob"])),
            publish: Some(AccessList::from_tokens(["$authenticated"])),
            unpublish: None,
        }],
        Some(AccessList::from_tokens(["$all"])),
    );
    let app = router_with_auth(config, auth_state);
    let response = app
        .clone()
        .oneshot(
            Request::post(format!(
                "/v2/acme/public/blobs/uploads/?mount={digest}&from=acme/secret",
            ))
            .header(header::AUTHORIZATION, &auth)
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        get(&app, &format!("/v2/acme/public/blobs/{digest}")).await.status(),
        StatusCode::NOT_FOUND,
    );
    let response = app
        .clone()
        .oneshot(
            Request::post(format!(
                "/v2/acme/public/blobs/uploads/?mount={digest}&from=acme/secret",
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    for path in [
        format!("/v2/acme/secret/referrers/{digest}"),
        "/v2/acme/secret/tags/list?n=1".into(),
        format!("/v2/acme/secret/blobs/{digest}"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::AUTHORIZATION, &auth)
                    .header(header::RANGE, "bytes=0-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let response = get(&app, "/v2/_catalog?n=1").await;
    assert!(!response.headers().contains_key(header::LINK));
    assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["repositories"], json!(["acme/public"]));
}

#[tokio::test]
async fn referrers_backfill_preexisting_manifests_on_both_backends() {
    for hosted_store in [
        pnpr::HostedStoreConfig::Fs,
        pnpr::HostedStoreConfig::ObjectStore {
            store: std::sync::Arc::new(object_store::memory::InMemory::new()),
            prefix: "legacy/".into(),
        },
    ] {
        let tmp = TempDir::new().unwrap();
        let mut config = oci_config(tmp.path().to_path_buf(), "$all");
        config.hosted_store = hosted_store;
        let storage = pnpr_storage::Storage::new(
            &config.hosted_store,
            config.storage.clone(),
            config.cache_storage.clone(),
        )
        .unwrap()
        .for_hosted("images");
        let app = router_with_auth(config, AuthState::in_memory());
        let auth = basic(&token(&app).await);
        let repository = repository_with_colliding_lock_keys();
        let subject = digest_of(b"subject");
        let manifest = json!({ "schemaVersion": 2, "mediaType": pnpr_oci::media_type::OCI_IMAGE_INDEX,
            "manifests": [], "subject": { "digest": subject, "size": 7 }, "artifactType": "application/example.sbom" });
        let response = app
            .clone()
            .oneshot(
                Request::put(format!("/v2/{repository}/manifests/sbom"))
                    .header(header::AUTHORIZATION, &auth)
                    .body(Body::from(manifest.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let key =
            pnpr_package_name::CanonicalPackageName::parse(&repository, Ecosystem::Oci).unwrap();
        let mut document: Value =
            serde_json::from_slice(&storage.read_hosted_document(&key).await.unwrap().unwrap())
                .unwrap();
        strip_referrer_metadata(&mut document, 1);
        storage
            .update_hosted_document_with_retry(&key, 1, |_| {
                Ok(Some(serde_json::to_vec(&document).unwrap()))
            })
            .await
            .unwrap();
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            get(&app, &format!("/v2/{repository}/referrers/{subject}")),
        )
        .await
        .expect("migration must not acquire the same stripe twice");
        assert_eq!(response.status(), StatusCode::OK);
        let payload: Value =
            serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        assert_eq!(payload["manifests"].as_array().unwrap().len(), 1);
        assert_eq!(payload["manifests"][0]["artifactType"], "application/example.sbom");
        let document = pnpr_oci::ImageDocument::parse(
            &storage.read_hosted_document(&key).await.unwrap().unwrap(),
        )
        .unwrap();
        assert_eq!(
            document.manifests()[0]
                .referrer
                .as_ref()
                .unwrap()
                .subject
                .as_ref()
                .unwrap()
                .to_string(),
            subject,
        );
    }
}

#[tokio::test]
async fn referrer_pages_bound_migration_and_keep_filter_and_registry() {
    for hosted_store in [
        pnpr::HostedStoreConfig::Fs,
        pnpr::HostedStoreConfig::ObjectStore {
            store: std::sync::Arc::new(object_store::memory::InMemory::new()),
            prefix: "pages/".into(),
        },
    ] {
        let tmp = TempDir::new().unwrap();
        let mut config = oci_config(tmp.path().to_path_buf(), "$all");
        config.hosted_store = hosted_store;
        let storage = pnpr_storage::Storage::new(
            &config.hosted_store,
            config.storage.clone(),
            config.cache_storage.clone(),
        )
        .unwrap()
        .for_hosted("images");
        let app = router_with_auth(config, AuthState::in_memory());
        let auth = basic(&token(&app).await);
        let subject = digest_of(b"subject");
        let mut expected = Vec::new();
        for index in 0..35 {
            let manifest = json!({ "schemaVersion": 2, "mediaType": pnpr_oci::media_type::OCI_IMAGE_INDEX,
                "manifests": [], "subject": { "digest": subject, "size": 7 },
                "artifactType": "application/example+json", "annotations": { "index": index.to_string() } }).to_string();
            expected.push(digest_of(manifest.as_bytes()));
            let response = app
                .clone()
                .oneshot(
                    Request::put(format!("/v2/acme/paged/manifests/referrer-{index}"))
                        .header(header::AUTHORIZATION, &auth)
                        .body(Body::from(manifest))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);
        }
        let key =
            pnpr_package_name::CanonicalPackageName::parse("acme/paged", Ecosystem::Oci).unwrap();
        let mut document: Value =
            serde_json::from_slice(&storage.read_hosted_document(&key).await.unwrap().unwrap())
                .unwrap();
        strip_referrer_metadata(&mut document, 35);
        storage
            .update_hosted_document_with_retry(&key, 1, |_| {
                Ok(Some(serde_json::to_vec(&document).unwrap()))
            })
            .await
            .unwrap();
        let path = format!("/oci/~images/v2/acme/paged/referrers/{subject}?artifactType=absent");
        let response = get(&app, &path).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key(header::LINK));
        let payload: Value =
            serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        assert_eq!(payload["manifests"], json!([]));
        let document = pnpr_oci::ImageDocument::parse(
            &storage.read_hosted_document(&key).await.unwrap().unwrap(),
        )
        .unwrap();
        assert_eq!(
            document.manifests().iter().filter(|entry| entry.referrer.is_some()).count(),
            32,
        );
        let first = get(&app, &path);
        let second = get(&app, &path);
        let responses: [_; 2] = tokio::join!(first, second).into();
        for response in responses {
            assert_eq!(response.status(), StatusCode::OK);
            assert!(!response.headers().contains_key(header::LINK));
        }
        let document = pnpr_oci::ImageDocument::parse(
            &storage.read_hosted_document(&key).await.unwrap().unwrap(),
        )
        .unwrap();
        assert!(document.manifests().iter().all(|entry| entry.referrer.is_some()));
        let mut path = format!(
            "/oci/~images/v2/acme/paged/referrers/{subject}?artifactType=application%2Fexample%2Bjson",
        );
        let mut received = Vec::new();
        let mut pages = 0;
        loop {
            let response = get(&app, &path).await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["oci-filters-applied"], "artifactType");
            let next = response.headers().get(header::LINK).map(|link| {
                let link = link.to_str().unwrap();
                assert!(link.contains("artifactType=application%2Fexample%2Bjson"));
                assert!(link.starts_with("</oci/~images/v2/"));
                link.strip_prefix('<').unwrap().split_once('>').unwrap().0.to_string()
            });
            let payload: Value =
                serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
            received.extend(
                payload["manifests"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|entry| entry["digest"].as_str().unwrap().to_string()),
            );
            pages += 1;
            assert!(pages <= 2);
            let Some(next) = next else { break };
            path = next;
        }
        expected.sort();
        assert_eq!(received, expected);
        assert_eq!(pages, 2);
    }
}

#[tokio::test]
async fn large_referrer_annotations_stay_out_of_repository_documents() {
    let tmp = TempDir::new().unwrap();
    let config = oci_config(tmp.path().to_path_buf(), "$all");
    let storage = pnpr_storage::Storage::new(
        &config.hosted_store,
        config.storage.clone(),
        config.cache_storage.clone(),
    )
    .unwrap()
    .for_hosted("images");
    let app = router_with_auth(config, AuthState::in_memory());
    let auth = basic(&token(&app).await);
    let subject = digest_of(b"subject");
    for index in 0..3 {
        let manifest = json!({ "schemaVersion": 2, "mediaType": pnpr_oci::media_type::OCI_IMAGE_INDEX,
            "manifests": [], "subject": { "digest": subject, "size": 7 },
            "annotations": { "large": "a".repeat(2 * 1024 * 1024), "index": index.to_string() } });
        let response = app
            .clone()
            .oneshot(
                Request::put(format!("/v2/acme/large/manifests/referrer-{index}"))
                    .header(header::AUTHORIZATION, &auth)
                    .body(Body::from(manifest.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }
    let key = pnpr_package_name::CanonicalPackageName::parse("acme/large", Ecosystem::Oci).unwrap();
    let document = storage.read_hosted_document(&key).await.unwrap().unwrap();
    assert!(document.len() < 2048, "annotations must not inflate ordinary repository reads");
    let mut path = format!("/v2/acme/large/referrers/{subject}");
    let mut count = 0;
    loop {
        let response = get(&app, &path).await;
        assert_eq!(response.status(), StatusCode::OK);
        let next = response.headers().get(header::LINK).map(|link| {
            link.to_str().unwrap().strip_prefix('<').unwrap().split_once('>').unwrap().0.to_string()
        });
        let bytes = body_bytes(response.into_body()).await;
        assert!(bytes.len() <= 4 * 1024 * 1024);
        let payload: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(payload["manifests"].as_array().unwrap().len(), 1);
        assert_eq!(
            payload["manifests"][0]["annotations"]["large"].as_str().unwrap().len(),
            2 * 1024 * 1024,
        );
        count += 1;
        assert!(count <= 3);
        let Some(next) = next else { break };
        path = next;
    }
    assert_eq!(count, 3);
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

#[tokio::test]
async fn referrer_migration_does_not_block_writers_or_restore_deleted_manifests() {
    let tmp = TempDir::new().unwrap();
    let objects = std::sync::Arc::new(pausing_store::PausingStore::default());
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.hosted_store = pnpr::HostedStoreConfig::ObjectStore {
        store: std::sync::Arc::<pausing_store::PausingStore>::clone(&objects),
        prefix: "migration/".into(),
    };
    let hosted = config.hosted.get_mut("images").unwrap();
    hosted.rules = std::mem::take(&mut hosted.rules)
        .with_default_unpublish(AccessList::from_tokens(["$authenticated"]));
    let storage = pnpr_storage::Storage::new(
        &config.hosted_store,
        config.storage.clone(),
        config.cache_storage.clone(),
    )
    .unwrap()
    .for_hosted("images");
    let app = router_with_auth(config, AuthState::in_memory());
    let auth = basic(&token(&app).await);
    let subject = digest_of(b"subject");
    let manifest = json!({ "schemaVersion": 2, "mediaType": pnpr_oci::media_type::OCI_IMAGE_INDEX,
        "manifests": [], "subject": { "digest": subject, "size": 7 } })
    .to_string();
    let digest = pnpr_oci::Digest::of(manifest.as_bytes());
    let response = app
        .clone()
        .oneshot(
            Request::put("/v2/acme/migration/manifests/sbom")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from(manifest))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let key =
        pnpr_package_name::CanonicalPackageName::parse("acme/migration", Ecosystem::Oci).unwrap();
    let mut document: Value =
        serde_json::from_slice(&storage.read_hosted_document(&key).await.unwrap().unwrap())
            .unwrap();
    strip_referrer_metadata(&mut document, 1);
    storage
        .update_hosted_document_with_retry(&key, 1, |_| {
            Ok(Some(serde_json::to_vec(&document).unwrap()))
        })
        .await
        .unwrap();
    objects.pause(digest.blob_filename());
    let reader = app.clone();
    let path = format!("/v2/acme/migration/referrers/{subject}");
    let read = tokio::spawn(async move { get(&reader, &path).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), objects.started.notified())
        .await
        .unwrap();
    let update = async {
        let published = push_image(&app, &auth, "acme/migration", "fresh").await;
        let deleted = app
            .clone()
            .oneshot(
                Request::delete(format!("/v2/acme/migration/manifests/{digest}"))
                    .header(header::AUTHORIZATION, &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(deleted.status(), StatusCode::ACCEPTED);
        published
    };
    let published = tokio::time::timeout(std::time::Duration::from_secs(5), update).await;
    objects.resume.notify_one();
    let published = published.expect("manifest reads must not hold the package writer lock");
    let response =
        tokio::time::timeout(std::time::Duration::from_secs(5), read).await.unwrap().unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let document =
        pnpr_oci::ImageDocument::parse(&storage.read_hosted_document(&key).await.unwrap().unwrap())
            .unwrap();
    assert!(document.manifest(&digest).is_none());
    assert_eq!(document.resolve("fresh").unwrap().digest.to_string(), published);
}

#[tokio::test]
async fn catalog_lists_repositories_under_published_and_blob_only_parents() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "latest").await;
    push_image(&app, &auth, "acme/app/tool", "latest").await;
    push_blob(&app, &auth, "acme/blobs", b"loose").await;
    push_image(&app, &auth, "acme/blobs/tool", "latest").await;
    let response = get(&app, "/v2/_catalog").await;
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["repositories"], json!(["acme/app", "acme/app/tool", "acme/blobs/tool"]));
}

#[tokio::test]
async fn unreferenced_blobs_can_be_deleted_and_uploaded_again() {
    let tmp = TempDir::new().unwrap();
    let app = app_allowing_deletes(&tmp);
    let auth = basic(&token(&app).await);
    let digest = push_blob(&app, &auth, "acme/app", b"config").await;
    let response = app
        .clone()
        .oneshot(
            Request::delete(format!("/v2/acme/app/blobs/{digest}"))
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        get(&app, &format!("/v2/acme/app/blobs/{digest}")).await.status(),
        StatusCode::NOT_FOUND,
    );
    push_image(&app, &auth, "acme/app", "after-delete").await;
    assert_eq!(get(&app, "/v2/acme/app/manifests/after-delete").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn configured_limits_bound_monolithic_resumable_and_manifest_uploads() {
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.oci.max_blob_bytes = 5;
    config.oci.max_manifest_bytes = 16;
    let app = router_with_auth(config, AuthState::in_memory());
    let auth = basic(&token(&app).await);
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/v2/acme/app/blobs/uploads/?digest={}", digest_of(b"123456")))
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from("123456"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = app
        .clone()
        .oneshot(
            Request::post("/v2/acme/app/blobs/uploads/")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from("123"))
                .unwrap(),
        )
        .await
        .unwrap();
    let location = response.headers()[header::LOCATION].to_str().unwrap().to_string();
    let response = app
        .clone()
        .oneshot(
            Request::patch(&location)
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from("456"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = app
        .clone()
        .oneshot(
            Request::put("/v2/acme/app/manifests/latest")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from(image_manifest("", &[])))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn batch_publishes_an_oci_manifest_and_rolls_back_on_invalid_siblings() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_blob(&app, &auth, "acme/app", b"config").await;
    let entry = json!({ "ecosystem": "oci", "name": "acme/app", "reference": "release", "manifest": STANDARD.encode(image_manifest("config", &[])) });
    let invalid = json!({ "ecosystem": "oci", "name": "acme/missing", "reference": "release", "manifest": STANDARD.encode(image_manifest("missing", &[])) });
    let response = app
        .clone()
        .oneshot(
            Request::put("/-/pnpr/v0/publish")
                .header(header::AUTHORIZATION, &auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "packages": [entry.clone(), invalid] }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(get(&app, "/v2/acme/app/manifests/release").await.status(), StatusCode::NOT_FOUND);
    let response = app
        .clone()
        .oneshot(
            Request::put("/-/pnpr/v0/publish")
                .header(header::AUTHORIZATION, &auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({ "packages": [entry] }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(get(&app, "/v2/acme/app/manifests/release").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn scoped_bearer_credentials_cannot_write_escape_repository_or_survive_revocation() {
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.oci.bearer_auth = true;
    let auth_state = AuthState::in_memory();
    let app = router_with_auth(config, auth_state.clone());
    let parent = token(&app).await;
    let auth = basic(&parent);
    push_image(&app, &auth, "acme/app", "latest").await;
    push_image(&app, &auth, "acme/other", "latest").await;
    let challenge = get(&app, "/v2/").await;
    assert!(challenge.headers()[header::WWW_AUTHENTICATE].to_str().unwrap().starts_with("Bearer "));
    let response = app
        .clone()
        .oneshot(
            Request::get("/v2/token?service=pnpr&scope=repository:acme/app:pull")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    let scoped = format!("Bearer {}", payload["token"].as_str().unwrap());
    for (method, path, expected) in [
        ("GET", "/v2/acme/app/manifests/latest", StatusCode::OK),
        ("GET", "/v2/acme/other/manifests/latest", StatusCode::UNAUTHORIZED),
        ("POST", "/v2/acme/app/blobs/uploads/", StatusCode::UNAUTHORIZED),
        ("GET", "/-/npm/v1/tokens", StatusCode::UNAUTHORIZED),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::AUTHORIZATION, &scoped)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{method} {path}");
    }
    auth_state.tokens.revoke_by_key(&sha256_hex(parent.as_bytes())).await.unwrap();
    let response = app
        .oneshot(
            Request::get("/v2/acme/app/manifests/latest")
                .header(header::AUTHORIZATION, scoped)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn proxy_verifies_and_caches_manifest_and_blob_content() {
    let mut upstream = mockito::Server::new_async().await;
    let manifest = image_manifest("config", &[]);
    let manifest_mock = upstream
        .mock("GET", "/v2/other/app/manifests/latest")
        .with_header("content-type", "application/vnd.oci.image.manifest.v1+json")
        .with_header("docker-content-digest", &digest_of(&manifest))
        .with_body(manifest.clone())
        .expect(1)
        .create_async()
        .await;
    let blob_mock = upstream
        .mock("GET", format!("/v2/other/app/blobs/{}", digest_of(b"config")).as_str())
        .with_body("config")
        .expect(1)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.upstreams.get_mut("dockerhub").unwrap().url = format!("{}/", upstream.url());
    let app = router_with_auth(config, AuthState::in_memory());
    for _ in 0..2 {
        let response = get(&app, "/v2/other/app/manifests/latest").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response.into_body()).await, manifest);
        let response = get(&app, &format!("/v2/other/app/blobs/{}", digest_of(b"config"))).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response.into_body()).await, b"config");
    }
    let response = app
        .clone()
        .oneshot(Request::head("/v2/other/app/manifests/latest").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_LENGTH], manifest.len().to_string());
    assert!(body_bytes(response.into_body()).await.is_empty());
    manifest_mock.assert_async().await;
    blob_mock.assert_async().await;
}

#[tokio::test]
async fn blob_deletion_fences_a_manifest_commit_on_another_replica() {
    let tmp = TempDir::new().unwrap();
    let objects = std::sync::Arc::new(pausing_store::PausingStore::default());
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.hosted_store = pnpr::HostedStoreConfig::ObjectStore {
        store: std::sync::Arc::clone(&objects) as std::sync::Arc<dyn object_store::ObjectStore>,
        prefix: "fenced/".into(),
    };
    let hosted = config.hosted.get_mut("images").unwrap();
    hosted.rules = std::mem::take(&mut hosted.rules)
        .with_default_unpublish(AccessList::from_tokens(["$authenticated"]));
    let auth_state = AuthState::in_memory();
    let first = router_with_auth(config.clone(), auth_state.clone());
    config.cache_storage = tmp.path().join("second-cache");
    let second = router_with_auth(config, auth_state);
    let auth = basic(&token(&first).await);
    let digest = push_blob(&first, &auth, "acme/app", b"config").await;
    objects.pause_write("package.json".to_string());
    let publish = tokio::spawn(
        first.oneshot(
            Request::put("/v2/acme/app/manifests/latest")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::from(image_manifest("config", &[])))
                .unwrap(),
        ),
    );
    tokio::time::timeout(std::time::Duration::from_secs(10), objects.started.notified())
        .await
        .unwrap();
    let deleted = second
        .clone()
        .oneshot(
            Request::delete(format!("/v2/acme/app/blobs/{digest}"))
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::ACCEPTED);
    objects.resume.notify_one();
    assert_eq!(publish.await.unwrap().unwrap().status(), StatusCode::CONFLICT);
    assert_eq!(get(&second, "/v2/acme/app/manifests/latest").await.status(), StatusCode::NOT_FOUND);
    push_image(&second, &auth, "acme/app", "retry").await;
}

#[tokio::test]
async fn proxy_does_not_cache_corrupt_content_or_download_blob_bodies_for_head() {
    let mut upstream = mockito::Server::new_async().await;
    let manifest = image_manifest("config", &[]);
    let corrupt_manifest = upstream
        .mock("GET", "/v2/other/app/manifests/latest")
        .with_header("docker-content-digest", &digest_of(b"different"))
        .with_body(manifest)
        .expect(2)
        .create_async()
        .await;
    let path = format!("/v2/other/app/blobs/{}", digest_of(b"config"));
    let corrupt_blob =
        upstream.mock("GET", path.as_str()).with_body("poison").expect(2).create_async().await;
    let head = upstream
        .mock("HEAD", path.as_str())
        .with_header("content-length", "6")
        .expect(1)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.upstreams.get_mut("dockerhub").unwrap().url = format!("{}/", upstream.url());
    let app = router_with_auth(config, AuthState::in_memory());
    let response =
        app.clone().oneshot(Request::head(&path).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_LENGTH], "6");
    assert!(body_bytes(response.into_body()).await.is_empty());
    for _ in 0..2 {
        assert_eq!(
            get(&app, "/v2/other/app/manifests/latest").await.status(),
            StatusCode::BAD_REQUEST,
        );
        let response = get(&app, &path).await;
        assert_eq!(body_bytes(response.into_body()).await, b"poison");
    }
    head.assert_async().await;
    corrupt_manifest.assert_async().await;
    corrupt_blob.assert_async().await;
}

#[tokio::test]
async fn deletion_requires_read_access_even_with_a_permissive_unpublish_rule() {
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "alice");
    let hosted = config.hosted.get_mut("images").unwrap();
    hosted.rules =
        std::mem::take(&mut hosted.rules).with_default_unpublish(AccessList::from_tokens(["$all"]));
    let app = router_with_auth(config, AuthState::in_memory());
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "latest").await;
    let digest = push_blob(&app, &auth, "acme/app", b"orphan").await;
    for path in
        ["/v2/acme/app/manifests/latest".to_string(), format!("/v2/acme/app/blobs/{digest}")]
    {
        let response =
            app.clone().oneshot(Request::delete(path).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    let response = app
        .oneshot(
            Request::get(format!("/v2/acme/app/blobs/{digest}"))
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn scoped_mount_without_source_pull_permission_falls_back_to_upload() {
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.oci.bearer_auth = true;
    let app = router_with_auth(config, AuthState::in_memory());
    let auth = basic(&token(&app).await);
    let digest = push_blob(&app, &auth, "acme/source", b"source layer").await;
    let response = app
        .clone()
        .oneshot(
            Request::get("/v2/token?service=pnpr&scope=repository:acme/dest:push")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    let scoped = format!("Bearer {}", payload["token"].as_str().unwrap());
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/v2/acme/dest/blobs/uploads/?mount={digest}&from=acme/source"))
                .header(header::AUTHORIZATION, &scoped)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let location = response.headers()[header::LOCATION].to_str().unwrap().to_string();
    let response = app
        .clone()
        .oneshot(
            Request::get(&location)
                .header(header::AUTHORIZATION, &scoped)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        get(&app, &format!("/v2/acme/dest/blobs/{digest}")).await.status(),
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn cold_manifest_head_forwards_headers_without_getting_or_caching_a_body() {
    for cache in [true, false] {
        let mut upstream = mockito::Server::new_async().await;
        let digest = digest_of(b"manifest");
        let head = upstream
            .mock("HEAD", "/v2/other/app/manifests/latest")
            .with_header("content-length", "123")
            .with_header("content-type", pnpr_oci::media_type::OCI_IMAGE_MANIFEST)
            .with_header("docker-content-digest", &digest)
            .expect(2)
            .create_async()
            .await;
        let tmp = TempDir::new().unwrap();
        let mut config = oci_config(tmp.path().to_path_buf(), "$all");
        let source = config.upstreams.get_mut("dockerhub").unwrap();
        source.url = format!("{}/", upstream.url());
        source.cache = cache;
        let app = router_with_auth(config, AuthState::in_memory());
        for _ in 0..2 {
            let response = app
                .clone()
                .oneshot(
                    Request::head("/v2/other/app/manifests/latest").body(Body::empty()).unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CONTENT_LENGTH], "123");
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                pnpr_oci::media_type::OCI_IMAGE_MANIFEST,
            );
            assert_eq!(response.headers()["docker-content-digest"], digest);
            assert!(body_bytes(response.into_body()).await.is_empty());
        }
        head.assert_async().await;
    }
}
