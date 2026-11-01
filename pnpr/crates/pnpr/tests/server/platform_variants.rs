//! Publishing a version with platform variants, whose archives declare the
//! platforms they run on.

use super::{
    AuthState, Body, Config, Ecosystem, PackagePattern, Registries, Registry, Request, ServiceExt,
    StatusCode, TempDir, body_bytes, body_json, config_for, gzip, header, hosted_with_access,
    router_with_auth, sha512_integrity, tar_of,
};
use crate::tokens::issue_token;
use serde_json::{Value, json};

fn acme_config(storage: std::path::PathBuf) -> Config {
    let mut config = config_for("http://127.0.0.1:1", storage);
    config.routing.hosted.insert("acme".to_string(), hosted_with_access("acme", "$all"));
    let graph = vec![
        ("npmjs".to_string(), Registry::Upstream { patterns: vec![] }),
        (
            "acme".to_string(),
            Registry::Hosted {
                patterns: vec![PackagePattern::parse("@acme/*", Ecosystem::Npm).unwrap()],
            },
        ),
        ("main".to_string(), Registry::Router { sources: vec!["acme".to_string()] }),
    ];
    config.routing.registries =
        Registries::new(graph.into_iter().collect(), Some("main".to_string()));
    config
}

/// A `.tgz` with `manifest` as its `package.json` and one file of `content`.
fn archive(manifest: &Value, content: &str) -> (Vec<u8>, Vec<u8>) {
    let manifest = serde_json::to_vec(manifest).unwrap();
    let tar = tar_of(&[
        ("package/package.json", manifest.as_slice()),
        ("package/index.js", content.as_bytes()),
    ]);
    let tgz = gzip(&tar);
    (tar, tgz)
}

fn primary_manifest() -> Value {
    json!({ "name": "@acme/fast", "version": "1.0.0", "main": "index.js" })
}

fn variant_manifest(platforms: &[&str], extra: &Value) -> Value {
    let mut manifest = primary_manifest();
    manifest["variant"] = json!({ "platforms": platforms });
    for (field, value) in extra.as_object().unwrap() {
        manifest[field] = value.clone();
    }
    manifest
}

/// A publish of the primary and `variants`, each `(label, .tgz)`.
fn publish_request(primary: &[u8], variants: &[(&str, &[u8])], token: &str) -> Request<Body> {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    let attachment = |data: &[u8]| {
        json!({
            "content_type": "application/octet-stream",
            "data": BASE64.encode(data),
            "length": data.len(),
        })
    };
    let mut attachments = serde_json::Map::new();
    attachments.insert("@acme/fast-1.0.0.tgz".into(), attachment(primary));
    let mut references = serde_json::Map::new();
    for (label, tgz) in variants {
        attachments.insert(format!("@acme/fast-1.0.0~{label}.tgz"), attachment(tgz));
        references.insert((*label).to_string(), Value::String(sha512_integrity(tgz)));
    }
    let mut manifest = primary_manifest();
    manifest["dist"] = json!({
        "tarball": "http://example.test/@acme/fast/-/fast-1.0.0.tgz",
        "integrity": sha512_integrity(primary),
    });
    manifest["pnprVariants"] = Value::Object(references);
    let body = json!({
        "name": "@acme/fast",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": manifest },
        "_attachments": attachments,
    });
    Request::put("/npm/~acme/@acme/fast")
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

async fn setup() -> (TempDir, axum::Router, String) {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(acme_config(tmp.path().to_path_buf()), auth);
    (tmp, app, token)
}

async fn get(app: &axum::Router, uri: &str, accept: Option<&str>) -> axum::response::Response {
    let mut request = Request::get(uri);
    if let Some(accept) = accept {
        request = request.header(header::ACCEPT, accept);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn archives_declare_their_platforms_and_pnpr_derives_the_tags() {
    let (_tmp, app, token) = setup().await;
    let (_, primary) = archive(&primary_manifest(), "primary");
    let (static_tar, static_tgz) =
        archive(&variant_manifest(&["linux-x64-musl", "linux-x64"], &json!({})), "static");
    let (_, darwin_tgz) = archive(
        &variant_manifest(
            &["darwin-arm64-macos_11_0"],
            &json!({ "dependencies": { "is-number": "7.0.0" } }),
        ),
        "darwin",
    );
    let response = app
        .clone()
        .oneshot(publish_request(
            &primary,
            &[("static", &static_tgz), ("mac", &darwin_tgz)],
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let document = body_json(
        get(&app, "/npm/~main/@acme%2Ffast", Some("application/vnd.pnpr.install-v2+json"))
            .await
            .into_body(),
    )
    .await;
    let variants = &document["versions"]["1.0.0"]["variants"];
    assert_eq!(variants[0]["tag"], "linux-x64+linux-x64-musl");
    assert_eq!(
        variants[0]["platforms"],
        json!([
            { "os": "linux", "cpu": "x64", "libc": "musl" },
            { "os": "linux", "cpu": "x64", "libc": "glibc" },
        ]),
    );
    assert_eq!(variants[0]["integrity"], sha512_integrity(&static_tar));
    assert_eq!(variants[0]["manifest"], json!({}));
    assert!(variants[0].get("label").is_none(), "the attachment label stays internal");
    assert_eq!(variants[1]["tag"], "darwin-arm64-macos_11_0");
    assert_eq!(
        variants[1]["platforms"],
        json!([{ "os": "darwin", "cpu": "arm64", "osVersion": "11.0" }]),
    );
    assert_eq!(variants[1]["manifest"], json!({ "dependencies": { "is-number": "7.0.0" } }));

    let npm = body_json(get(&app, "/npm/~main/@acme%2Ffast", None).await.into_body()).await;
    assert!(npm["versions"]["1.0.0"].get("_pnprVariants").is_none());
    assert!(npm["versions"]["1.0.0"]["dist"].get("pnpr").is_none());

    let response =
        get(&app, "/npm/~main/@acme/fast@1.0.0/variants/linux-x64+linux-x64-musl", None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, static_tar);
    let selected =
        get(&app, "/npm/~main/@acme/fast@1.0.0+r0/variants/linux-x64+linux-x64-musl", None).await;
    assert_eq!(selected.status(), StatusCode::OK, "the selected revision's route serves it");
    let other =
        get(&app, "/npm/~main/@acme/fast@1.0.0+r1/variants/linux-x64+linux-x64-musl", None).await;
    assert_eq!(other.status(), StatusCode::NOT_FOUND);
    let unknown = get(&app, "/npm/~main/@acme/fast@1.0.0/variants/static", None).await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
}

async fn rejection(variants: &[(&str, Value)]) -> String {
    let (_tmp, app, token) = setup().await;
    let (_, primary) = archive(&primary_manifest(), "primary");
    let archives: Vec<(&str, Vec<u8>)> = variants
        .iter()
        .map(|(label, manifest)| (*label, archive(manifest, label).1))
        .collect();
    let refs: Vec<(&str, &[u8])> = archives
        .iter()
        .map(|(label, tgz)| (*label, tgz.as_slice()))
        .collect();
    let response = app
        .clone()
        .oneshot(publish_request(&primary, &refs, &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    String::from_utf8(body_bytes(response.into_body()).await).unwrap()
}

#[tokio::test]
async fn publication_rejects_variants_that_break_an_invariant() {
    let cases = [
        (
            "peers",
            vec![(
                "a",
                variant_manifest(&["linux-x64"], &json!({ "peerDependencies": { "react": "*" } })),
            )],
        ),
        ("os", vec![("a", variant_manifest(&["linux-x64"], &json!({ "os": ["linux"] })))]),
        ("non-canonical", vec![("a", variant_manifest(&["linux-x64-glibc"], &json!({})))]),
        ("no field", vec![("a", primary_manifest())]),
        (
            "overlap",
            vec![
                ("a", variant_manifest(&["linux-x64"], &json!({}))),
                ("b", variant_manifest(&["linux-x64", "linux-x64-musl"], &json!({}))),
            ],
        ),
        (
            "build",
            vec![(
                "a",
                variant_manifest(&["linux-x64"], &json!({ "scripts": { "postinstall": "x" } })),
            )],
        ),
    ];
    for (case, variants) in cases {
        let reason = rejection(&variants).await;
        assert!(reason.contains("variant"), "{case}: {reason}");
    }
}
