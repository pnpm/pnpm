//! The OCI distribution surface: the version check, blob upload in one
//! request and in chunks, manifests, tags, the catalog, and the access rules
//! every one of them goes through.

// `#[path]` rather than the `tests/common/mod.rs` layout, which the
// Perfectionist dylint forbids.
#[path = "common/ecosystem.rs"]
#[allow(
    dead_code,
    reason = "the shared fixtures also carry the upstream-proxy helpers the Cargo and Python               surfaces use, which this surface has no half of yet"
)]
mod common;

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
    for reference in ["sha256:short", ".leading-dot", "has%2Fslash"] {
        let request = Request::put(format!("/v2/acme/app/manifests/{reference}"))
            .header(header::AUTHORIZATION, &auth)
            .header(header::CONTENT_TYPE, "application/vnd.oci.image.manifest.v1+json")
            .body(Body::from(image_manifest("config", &["layer"])))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_ne!(response.status(), StatusCode::CREATED, "{reference} should not be a tag");
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
async fn deleting_a_blob_is_not_offered() {
    let tmp = TempDir::new().unwrap();
    let app = app_allowing_deletes(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;

    // Nothing tracks which manifests reference a blob, so removing one would
    // leave the repository advertising an image that cannot be pulled.
    let request = Request::delete(format!("/v2/acme/app/blobs/{}", digest_of(b"layer")))
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
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
        assert_ne!(response.status(), StatusCode::ACCEPTED, "{range}");
    }
}
