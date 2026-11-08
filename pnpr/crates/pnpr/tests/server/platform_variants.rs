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

fn any_manifest() -> Value {
    json!({ "name": "@acme/fast", "version": "1.0.0", "main": "index.js" })
}

fn variant_manifest(platforms: &[&str], extra: &Value) -> Value {
    let mut manifest = any_manifest();
    manifest["variant"] = json!({ "platforms": platforms });
    for (field, value) in extra.as_object().unwrap() {
        manifest[field] = value.clone();
    }
    manifest
}

/// A publish of the `any` archive and `variants`, each `(tag, .tgz)`.
fn publish_request(any_tgz: &[u8], variants: &[(&str, &[u8])], token: &str) -> Request<Body> {
    publish_request_with(Some(any_tgz), variants, token)
}

/// [`publish_request`], with the `any` archive attached only when given:
/// the version's manifest still names its integrity.
fn publish_request_with(
    any_tgz: Option<&[u8]>,
    variants: &[(&str, &[u8])],
    token: &str,
) -> Request<Body> {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    let attachment = |data: &[u8]| {
        json!({
            "content_type": "application/octet-stream",
            "data": BASE64.encode(data),
            "length": data.len(),
        })
    };
    let mut attachments = serde_json::Map::new();
    let (_, default_any) = archive(&any_manifest(), "any");
    let integrity = sha512_integrity(any_tgz.unwrap_or(&default_any));
    if let Some(any_tgz) = any_tgz {
        attachments.insert("@acme/fast-1.0.0.tgz".into(), attachment(any_tgz));
    }
    for (tag, tgz) in variants {
        let mut variant = attachment(tgz);
        variant["integrity"] = Value::String(sha512_integrity(tgz));
        attachments.insert(format!("@acme/fast-1.0.0~{tag}.tgz"), variant);
    }
    let mut manifest = any_manifest();
    manifest["dist"] = json!({
        "tarball": "http://example.test/@acme/fast/-/fast-1.0.0.tgz",
        "integrity": integrity,
    });
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
    let (_, any_tgz) = archive(&any_manifest(), "any");
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
            &any_tgz,
            &[("linux-x64+linux-x64-musl", &static_tgz), ("darwin-arm64-macos_11_0", &darwin_tgz)],
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
    let unknown = get(&app, "/npm/~main/@acme/fast@1.0.0/variants/linux-arm64", None).await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
}

async fn rejection(variants: &[(&str, Value)]) -> String {
    let (_tmp, app, token) = setup().await;
    let (_, any_tgz) = archive(&any_manifest(), "any");
    let archives: Vec<(&str, Vec<u8>)> = variants
        .iter()
        .map(|(tag, manifest)| (*tag, archive(manifest, tag).1))
        .collect();
    let refs: Vec<(&str, &[u8])> = archives
        .iter()
        .map(|(tag, tgz)| (*tag, tgz.as_slice()))
        .collect();
    let response = app
        .clone()
        .oneshot(publish_request(&any_tgz, &refs, &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    body_json(response.into_body()).await["error"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn publication_rejects_variants_that_break_an_invariant() {
    let cases = [
        (
            "peers",
            r#"its "peerDependencies" differs from the version's"#,
            vec![(
                "linux-x64",
                variant_manifest(&["linux-x64"], &json!({ "peerDependencies": { "react": "*" } })),
            )],
        ),
        (
            "os",
            r#"it declares "os" beside its variant field"#,
            vec![("linux-x64", variant_manifest(&["linux-x64"], &json!({ "os": ["linux"] })))],
        ),
        (
            "non-canonical",
            r#"platform tag "linux-x64-glibc" has an unknown or misplaced part"#,
            vec![("linux-x64", variant_manifest(&["linux-x64-glibc"], &json!({})))],
        ),
        ("no field", "its package.json has no variant field", vec![("linux-x64", any_manifest())]),
        (
            "overlap",
            r#"variants "linux-x64" and "linux-x64+linux-x64-musl" of 1.0.0 both list linux-x64"#,
            vec![
                ("linux-x64", variant_manifest(&["linux-x64"], &json!({}))),
                (
                    "linux-x64+linux-x64-musl",
                    variant_manifest(&["linux-x64", "linux-x64-musl"], &json!({})),
                ),
            ],
        ),
        (
            "build",
            "it requires a build",
            vec![(
                "linux-x64",
                variant_manifest(&["linux-x64"], &json!({ "scripts": { "postinstall": "x" } })),
            )],
        ),
        (
            "another tag",
            r#"variant "linux-x64" of 1.0.0: its variant field gives the tag "linux-x64-musl""#,
            vec![("linux-x64", variant_manifest(&["linux-x64-musl"], &json!({})))],
        ),
        (
            "unsorted tag",
            r#"its variant field gives the tag "linux-x64+linux-x64-musl""#,
            vec![(
                "linux-x64-musl+linux-x64",
                variant_manifest(&["linux-x64", "linux-x64-musl"], &json!({})),
            )],
        ),
    ];
    for (case, expected, variants) in cases {
        let reason = rejection(&variants).await;
        assert!(reason.contains(expected), "{case}: {reason}");
    }
}

/// With `SQLite` metadata, a publication commits atomically, and the stored
/// variants survive the round trip through the database.
#[tokio::test]
async fn variants_are_served_from_sqlite_metadata() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let mut config = acme_config(tmp.path().to_path_buf());
    config.storage.metadata =
        pnpr_config::MetadataBackend::Sqlite(tmp.path().join("metadata.sqlite"));
    let app = router_with_auth(config, auth);
    let (_, any_tgz) = archive(&any_manifest(), "any");
    let (musl_tar, musl_tgz) = archive(&variant_manifest(&["linux-x64-musl"], &json!({})), "musl");
    let response = app
        .clone()
        .oneshot(publish_request(&any_tgz, &[("linux-x64-musl", &musl_tgz)], &token))
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
    assert_eq!(variants[0]["tag"], "linux-x64-musl");
    assert_eq!(variants[0]["integrity"], sha512_integrity(&musl_tar));

    // The version's release record lists each variant with the inputs its
    // manifest was derived from.
    let record =
        body_json(get(&app, "/npm/~main/@acme%2Ffast/1.0.0+r0", None).await.into_body()).await;
    let variant = &record["variants"][0];
    assert_eq!(variant["tag"], "linux-x64-musl");
    assert_eq!(variant["integrity"], sha512_integrity(&musl_tar));
    assert_eq!(
        variant["inputs"]["archive"],
        json!({ "kind": "tgz", "integrity": sha512_integrity(&musl_tgz) }),
    );
    assert_eq!(variant["inputs"]["packageJson"]["path"], "package/package.json");
    assert_eq!(variant["purl"], "pkg:npm/%40acme/fast@1.0.0?variant=linux-x64-musl");

    // The scoped route, and the encoded name whose three segments it shares
    // with a release note's route.
    for uri in [
        "/npm/~main/@acme/fast@1.0.0/variants/linux-x64-musl",
        "/npm/~main/@acme%2Ffast@1.0.0/variants/linux-x64-musl",
    ] {
        let response = get(&app, uri, None).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        assert_eq!(body_bytes(response.into_body()).await, musl_tar, "{uri}");
    }
}

/// An archive with two root manifests is refused: a variant is checked
/// against the one `package.json` a client installs.
#[tokio::test]
async fn a_variant_is_checked_against_the_manifest_clients_install() {
    let (_tmp, app, token) = setup().await;
    let (_, any_tgz) = archive(&any_manifest(), "any");
    let declared = serde_json::to_vec(&variant_manifest(&["linux-x64"], &json!({}))).unwrap();
    let installed = serde_json::to_vec(&any_manifest()).unwrap();
    let tgz = gzip(&tar_of(&[
        ("package/package.json", declared.as_slice()),
        ("package/index.js", b"variant".as_slice()),
        ("package/package.json", installed.as_slice()),
    ]));
    let response = app
        .clone()
        .oneshot(publish_request(&any_tgz, &[("linux-x64", &tgz)], &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let reason = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(reason.contains("ambiguous"), "{reason}");
}

/// npm's documents leave the stored variants out, and a deprecation writes
/// one of them back: the store keeps the variants of every version it
/// retains.
#[tokio::test]
async fn a_rewrite_from_npms_document_keeps_the_variants() {
    let (_tmp, app, token) = setup().await;
    let (_, any_tgz) = archive(&any_manifest(), "any");
    let (musl_tar, musl_tgz) = archive(&variant_manifest(&["linux-x64-musl"], &json!({})), "musl");
    let response = app
        .clone()
        .oneshot(publish_request(&any_tgz, &[("linux-x64-musl", &musl_tgz)], &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let mut document =
        body_json(get(&app, "/npm/~acme/@acme%2Ffast", None).await.into_body()).await;
    assert!(document["versions"]["1.0.0"].get("_pnprVariants").is_none());
    document["versions"]["1.0.0"]["deprecated"] = json!("use 2");
    let rewrite = Request::put(format!(
        "/npm/~acme/@acme%2Ffast/-rev/{}",
        document["_rev"].as_str().unwrap(),
    ))
    .header("content-type", "application/json")
    .header(header::AUTHORIZATION, format!("Bearer {token}"))
    .body(Body::from(serde_json::to_vec(&document).unwrap()))
    .unwrap();
    let response = app
        .clone()
        .oneshot(rewrite)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let response = get(&app, "/npm/~main/@acme/fast@1.0.0/variants/linux-x64-musl", None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, musl_tar);
}

/// The variant set is part of the immutable version: variants come only
/// with their version's `any` archive, whose own manifest has to describe
/// the published version.
#[tokio::test]
async fn variants_come_only_with_an_any_archive_they_agree_with() {
    let (_tmp, app, token) = setup().await;
    let (_, any_tgz) = archive(&any_manifest(), "any");
    let response = app
        .clone()
        .oneshot(publish_request(&any_tgz, &[], &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let (_, musl) = archive(&variant_manifest(&["linux-x64-musl"], &json!({})), "musl");
    let response = app
        .clone()
        .oneshot(publish_request_with(None, &[("linux-x64-musl", &musl)], &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST, "variants for a hosted version");
    let reason = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(reason.contains("only with its `any` archive"), "{reason}");

    let (_tmp, app, token) = setup().await;
    let mut peers = any_manifest();
    peers["peerDependencies"] = json!({ "react": "*" });
    let (_, any_tgz) = archive(&peers, "any");
    let response = app
        .clone()
        .oneshot(publish_request(&any_tgz, &[("linux-x64-musl", &musl)], &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let reason = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(reason.contains("peerDependencies"), "{reason}");

    let (_tmp, app, token) = setup().await;
    for (field, value) in [("version", "2.0.0"), ("name", "@acme/other")] {
        let mut renamed = any_manifest();
        renamed[field] = json!(value);
        let (_, any_tgz) = archive(&renamed, "any");
        let response = app
            .clone()
            .oneshot(publish_request(&any_tgz, &[("linux-x64-musl", &musl)], &token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{field}");
        let reason = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
        assert!(reason.contains(field), "{field}: {reason}");
    }
}

/// Stage `tgz` for the caller `token` names, as a publisher does before a
/// commit that references it.
async fn stage(app: &axum::Router, tgz: &[u8], token: &str) {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("integrity", &sha512_integrity(tgz))
        .finish();
    let path = format!("/npm/-/pnpr/v0/artifacts/blob?{query}");
    let response = app
        .clone()
        .oneshot(
            Request::put(path)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_LENGTH, tgz.len())
                .body(Body::from(tgz.to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn a_batch_publishes_a_version_and_its_variants_from_staged_archives() {
    let (_tmp, app, token) = setup().await;
    let (_, any_tgz) = archive(&any_manifest(), "any");
    let (static_tar, static_tgz) =
        archive(&variant_manifest(&["linux-x64-musl", "linux-x64"], &json!({})), "static");
    let (_, darwin_tgz) = archive(&variant_manifest(&["darwin-arm64"], &json!({})), "darwin");
    for tgz in [&any_tgz, &static_tgz, &darwin_tgz] {
        stage(&app, tgz, &token).await;
    }
    // The single-package document, with each attachment naming its staged
    // archive instead of carrying it, as one entry of a batch.
    let request = publish_request(
        &any_tgz,
        &[("linux-x64+linux-x64-musl", &static_tgz), ("darwin-arm64", &darwin_tgz)],
        &token,
    );
    let mut document: Value =
        serde_json::from_slice(&body_bytes(request.into_body()).await).unwrap();
    for attachment in document["_attachments"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        let data = attachment["data"].as_str().unwrap();
        let bytes =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data).unwrap();
        *attachment = json!({ "integrity": sha512_integrity(&bytes), "length": bytes.len() });
    }
    let batch = json!({ "packages": [document] });
    let response = app
        .clone()
        .oneshot(
            Request::put("/-/pnpr/v0/publish")
                .header("content-type", "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(serde_json::to_vec(&batch).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let response =
        get(&app, "/npm/~main/@acme/fast@1.0.0/variants/linux-x64+linux-x64-musl", None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, static_tar);
    let any_tarball = get(&app, "/npm/~main/@acme/fast/-/fast-1.0.0.tgz", None).await;
    assert_eq!(body_bytes(any_tarball.into_body()).await, any_tgz);
}

/// A tag names one route segment and one attachment, so pnpr refuses a tag
/// longer than the limit before it reads any archive.
#[tokio::test]
async fn a_tag_over_the_limit_is_refused() {
    let platforms: Vec<String> = (0..12)
        .map(|abi| format!("linux-x64-manylinux_2_17-node{}", 100 + abi))
        .collect();
    let platforms: Vec<&str> = platforms
        .iter()
        .map(String::as_str)
        .collect();
    let tag = platforms.join("+");
    assert!(tag.len() > pnpm_platform_variants::MAX_VARIANT_TAG_BYTES);
    let reason = rejection(&[(&tag, variant_manifest(&platforms, &json!({})))]).await;
    assert!(reason.contains("over the 128-byte limit"), "{reason}");
}

/// A variant attachment names its version, which the publish must describe,
/// and declares the integrity its archive is verified against.
#[tokio::test]
async fn a_variant_attachment_names_a_described_version_and_its_integrity() {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    let (_, any_tgz) = archive(&any_manifest(), "any");
    let (_, musl) = archive(&variant_manifest(&["linux-x64-musl"], &json!({})), "musl");
    for (case, filename, attachment, expected) in [
        (
            "another version",
            "@acme/fast-2.0.0~linux-x64-musl.tgz",
            json!({ "data": BASE64.encode(&musl), "integrity": sha512_integrity(&musl) }),
            "names version 2.0.0, which the publish does not describe",
        ),
        (
            "no integrity",
            "@acme/fast-1.0.0~linux-x64-musl.tgz",
            json!({ "data": BASE64.encode(&musl) }),
            "has no integrity",
        ),
        (
            "other bytes",
            "@acme/fast-1.0.0~linux-x64-musl.tgz",
            json!({ "data": BASE64.encode(&musl), "integrity": sha512_integrity(&any_tgz) }),
            "EINTEGRITY",
        ),
    ] {
        let (_tmp, app, token) = setup().await;
        let request = publish_request(&any_tgz, &[], &token);
        let (parts, body) = request.into_parts();
        let mut document: Value = serde_json::from_slice(&body_bytes(body).await).unwrap();
        document["_attachments"][filename] = attachment;
        let request =
            Request::from_parts(parts, Body::from(serde_json::to_vec(&document).unwrap()));
        let response = app
            .clone()
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{case}");
        let reason = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
        assert!(reason.contains(expected), "{case}: {reason}");
    }
}

/// The version's `any` archive may declare its integrity beside its bytes, as
/// a variant does, and it must be the version's `dist.integrity`. A sigstore
/// bundle declares none.
#[tokio::test]
async fn an_archive_declares_its_versions_integrity_or_none() {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    let (_, any_tgz) = archive(&any_manifest(), "any");
    let (_, other) = archive(&any_manifest(), "other");
    for (case, filename, attachment, expected) in [
        (
            "other integrity",
            "@acme/fast-1.0.0.tgz",
            json!({ "data": BASE64.encode(&any_tgz), "integrity": sha512_integrity(&other) }),
            "its version's dist.integrity",
        ),
        (
            "sigstore with integrity",
            "@acme/fast-1.0.0.sigstore",
            json!({ "data": "{}", "integrity": sha512_integrity(&other) }),
            "with no `integrity`",
        ),
    ] {
        let (_tmp, app, token) = setup().await;
        let request = publish_request(&any_tgz, &[], &token);
        let (parts, body) = request.into_parts();
        let mut document: Value = serde_json::from_slice(&body_bytes(body).await).unwrap();
        document["_attachments"][filename] = attachment;
        let request =
            Request::from_parts(parts, Body::from(serde_json::to_vec(&document).unwrap()));
        let response = app
            .clone()
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{case}");
        let reason = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
        assert!(reason.contains(expected), "{case}: {reason}");
    }

    let (_tmp, app, token) = setup().await;
    let request = publish_request(&any_tgz, &[], &token);
    let (parts, body) = request.into_parts();
    let mut document: Value = serde_json::from_slice(&body_bytes(body).await).unwrap();
    document["_attachments"]["@acme/fast-1.0.0.tgz"]["integrity"] =
        json!(sha512_integrity(&any_tgz));
    let request = Request::from_parts(parts, Body::from(serde_json::to_vec(&document).unwrap()));
    assert_eq!(
        app.oneshot(request).await
            .unwrap()
            .status(),
        StatusCode::CREATED,
    );
}

/// `request` with a provenance statement naming `subjects`, each a PURL and
/// the `.tgz` it names.
async fn with_statement(request: Request<Body>, subjects: &[(&str, &[u8])]) -> Request<Body> {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use sha2::Digest as _;
    use std::fmt::Write as _;

    let (parts, body) = request.into_parts();
    let mut document: Value = serde_json::from_slice(&body_bytes(body).await).unwrap();
    let subject: Vec<Value> = subjects
        .iter()
        .map(|(purl, tgz)| {
            let hex: String = sha2::Sha512::digest(tgz)
                .iter()
                .fold(String::new(), |mut hex, byte| {
                    let _ = write!(hex, "{byte:02x}");
                    hex
                });
            json!({ "name": purl, "digest": { "sha512": hex } })
        })
        .collect();
    let statement = json!({
        "_type": "https://in-toto.io/Statement/v1",
        "subject": subject,
        "predicateType": "https://slsa.dev/provenance/v1",
        "predicate": {},
    });
    let bundle = json!({
        "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
        "dsseEnvelope": {
            "payload": BASE64.encode(statement.to_string()),
            "payloadType": "application/vnd.in-toto+json",
            "signatures": [],
        },
    })
    .to_string();
    document["_attachments"]["@acme/fast-1.0.0.sigstore"] = json!({
        "content_type": "application/vnd.dev.sigstore.bundle.v0.3+json",
        "data": bundle,
        "length": bundle.len(),
    });
    Request::from_parts(parts, Body::from(serde_json::to_vec(&document).unwrap()))
}

/// The version's provenance statement names every archive it publishes, a
/// variant's by its PURL with the `variant` qualifier and `+` as `%2B`.
#[tokio::test]
async fn a_statement_names_every_archive_of_the_version() {
    let (_, any_tgz) = archive(&any_manifest(), "any");
    let (_, static_tgz) =
        archive(&variant_manifest(&["linux-x64", "linux-x64-musl"], &json!({})), "static");
    let variants: &[(&str, &[u8])] = &[("linux-x64+linux-x64-musl", &static_tgz)];
    let any_subject = ("pkg:npm/%40acme/fast@1.0.0", any_tgz.as_slice());
    let variant_subject =
        ("pkg:npm/%40acme/fast@1.0.0?variant=linux-x64%2Blinux-x64-musl", static_tgz.as_slice());

    for (case, subjects) in [
        ("any archive only", vec![any_subject]),
        (
            "bare +",
            vec![
                any_subject,
                (
                    "pkg:npm/%40acme/fast@1.0.0?variant=linux-x64+linux-x64-musl",
                    static_tgz.as_slice(),
                ),
            ],
        ),
    ] {
        let (_tmp, app, token) = setup().await;
        let request = with_statement(publish_request(&any_tgz, variants, &token), &subjects).await;
        let response = app
            .clone()
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{case}");
        let reason = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
        assert!(reason.contains("does not name pkg:npm/%40acme/fast@1.0.0?variant="), "{reason}");
    }

    let (_tmp, app, token) = setup().await;
    let request = with_statement(
        publish_request(&any_tgz, variants, &token),
        &[any_subject, variant_subject],
    )
    .await;
    let response = app
        .clone()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

/// A version published with variants only: its document has no `dist`, and
/// no attachment of an `any` archive.
fn variants_only_request(base: &Value, variants: &[(&str, &[u8])], token: &str) -> Request<Body> {
    variants_only_request_at("1.0.0", base, variants, token)
}

/// [`variants_only_request`] of the version `key`.
fn variants_only_request_at(
    key: &str,
    base: &Value,
    variants: &[(&str, &[u8])],
    token: &str,
) -> Request<Body> {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    let mut attachments = serde_json::Map::new();
    for (tag, tgz) in variants {
        attachments.insert(
            format!("@acme/fast-{key}~{tag}.tgz"),
            json!({
                "data": BASE64.encode(tgz),
                "integrity": sha512_integrity(tgz),
                "length": tgz.len(),
            }),
        );
    }
    let body = json!({
        "name": "@acme/fast",
        "dist-tags": { "latest": key },
        "versions": { key: base },
        "_attachments": attachments,
    });
    Request::put("/npm/~acme/@acme/fast")
        .header("content-type", "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

#[tokio::test]
async fn a_version_without_an_any_archive_rejects_a_non_semver_version() {
    let (tmp, app, token) = setup().await;
    let (_, linux) = archive(&variant_manifest(&["linux-x64"], &json!({})), "linux");
    let mut manifest = any_manifest();
    manifest["version"] = json!("banana");
    let response = app
        .oneshot(variants_only_request_at("banana", &manifest, &[("linux-x64", &linux)], &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let reason = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(reason.contains("semver"), "{reason}");
    assert!(
        pnpr_fixtures::read_hosted_document(
            tmp.path(),
            "acme",
            pnpr_storage::Ecosystem::Npm,
            "@acme/fast",
        )
        .is_none(),
    );
}

/// A version published without an `any` archive: npm's documents describe
/// the minimal archive pnpr generates, which lists where the variants run,
/// and pnpr's own document and release record describe only the variants.
#[tokio::test]
async fn a_version_without_an_any_archive_gets_a_generated_one_for_npm() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let mut config = acme_config(tmp.path().to_path_buf());
    config.storage.metadata =
        pnpr_config::MetadataBackend::Sqlite(tmp.path().join("metadata.sqlite"));
    let app = router_with_auth(config, auth);

    let mut base = any_manifest();
    base["dependencies"] = json!({ "shared": "1.0.0" });
    let deps = |extra: &Value| {
        let mut dependencies = json!({ "shared": "1.0.0" });
        for (name, spec) in extra.as_object().unwrap() {
            dependencies[name] = spec.clone();
        }
        json!({ "dependencies": dependencies })
    };
    let (_, linux) = archive(
        &variant_manifest(
            &["linux-x64", "linux-arm64-musl"],
            &deps(&json!({ "only-linux": "2.0.0" })),
        ),
        "linux",
    );
    let (_, darwin) = archive(&variant_manifest(&["darwin-arm64"], &deps(&json!({}))), "darwin");
    let response = app
        .clone()
        .oneshot(variants_only_request(
            &base,
            &[("linux-arm64-musl+linux-x64", &linux), ("darwin-arm64", &darwin)],
            &token,
        ))
        .await
        .unwrap();
    let status = response.status();
    let body = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let npm = body_json(get(&app, "/npm/~main/@acme%2Ffast", None).await.into_body()).await;
    let version = &npm["versions"]["1.0.0"];
    assert_eq!(version["os"], json!(["darwin", "linux"]));
    assert_eq!(version["cpu"], json!(["arm64", "x64"]));
    assert_eq!(version["libc"], json!(["glibc", "musl"]));
    assert!(version.get("dependencies").is_none(), "{version}");
    let tarball = get(&app, "/npm/~main/@acme/fast/-/fast-1.0.0.tgz", None).await;
    assert_eq!(tarball.status(), StatusCode::OK);
    let tgz = body_bytes(tarball.into_body()).await;
    assert_eq!(sha512_integrity(&tgz), version["dist"]["integrity"]);

    let document = body_json(
        get(&app, "/npm/~main/@acme%2Ffast", Some("application/vnd.pnpr.install-v2+json"))
            .await
            .into_body(),
    )
    .await;
    let entry = &document["versions"]["1.0.0"];
    assert!(entry.get("integrity").is_none(), "{entry}");
    assert_eq!(entry["manifest"], json!({ "dependencies": { "shared": "1.0.0" } }));
    let variants = &entry["variants"];
    assert_eq!(variants[0]["tag"], "linux-arm64-musl+linux-x64");
    assert_eq!(
        variants[0]["manifest"],
        json!({
            "dependencies": { "shared": "1.0.0", "only-linux": "2.0.0" },
            "hasInstallScript": false,
        }),
    );

    let record =
        body_json(get(&app, "/npm/~main/@acme%2Ffast/1.0.0+r0", None).await.into_body()).await;
    assert!(record.get("integrity").is_none(), "{record}");
    assert!(record.get("inputs").is_none(), "{record}");
    assert_eq!(record["variants"][1]["purl"], "pkg:npm/%40acme/fast@1.0.0?variant=darwin-arm64");
}

/// A release job that packs no `any` archive sends the version with only
/// `dist.integrity`, naming the `any` archive another job staged.
#[tokio::test]
async fn a_staged_any_archive_is_referenced_by_its_integrity_alone() {
    let (_tmp, app, token) = setup().await;
    let (_, any_tgz) = archive(&any_manifest(), "any");
    let (_, musl) = archive(&variant_manifest(&["linux-x64-musl"], &json!({})), "musl");
    for tgz in [&any_tgz, &musl] {
        stage(&app, tgz, &token).await;
    }
    let mut manifest = any_manifest();
    manifest["dist"] = json!({ "integrity": sha512_integrity(&any_tgz) });
    let attachment =
        |tgz: &[u8]| json!({ "integrity": sha512_integrity(tgz), "length": tgz.len() });
    let document = json!({
        "name": "@acme/fast",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": manifest },
        "_attachments": {
            "@acme/fast-1.0.0.tgz": attachment(&any_tgz),
            "@acme/fast-1.0.0~linux-x64-musl.tgz": attachment(&musl),
        },
    });
    let response = app
        .clone()
        .oneshot(
            Request::put("/npm/-/pnpr/v0/publish")
                .header("content-type", "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(serde_json::to_vec(&json!({ "packages": [document] })).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let tarball = get(&app, "/npm/~main/@acme/fast/-/fast-1.0.0.tgz", None).await;
    assert_eq!(tarball.status(), StatusCode::OK);
    assert_eq!(body_bytes(tarball.into_body()).await, any_tgz);
}

/// Publish `request` to a fresh registry and give its status and body.
async fn publish_once(request: impl FnOnce(&str) -> Request<Body>) -> (StatusCode, String) {
    let (_tmp, app, token) = setup().await;
    let response = app
        .oneshot(request(&token))
        .await
        .unwrap();
    let status = response.status();
    (status, String::from_utf8(body_bytes(response.into_body()).await).unwrap())
}

/// A version published without an `any` archive is still the version its
/// key names, of the package its URL names: neither its manifest nor a
/// variant's may name another, and its key carries no build metadata, which
/// pnpm reserves for registry revisions.
#[tokio::test]
async fn a_version_without_an_any_archive_names_its_own_package_and_version() {
    let (_, linux) = archive(&variant_manifest(&["linux-x64"], &json!({})), "linux");
    let (status, body) = publish_once(|token| {
        variants_only_request_at("1.0.0+r1", &any_manifest(), &[("linux-x64", &linux)], token)
    })
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let renamed = |field: &str, value: &str| {
        let mut manifest = any_manifest();
        manifest[field] = json!(value);
        manifest
    };
    let (_, linux2) =
        archive(&variant_manifest(&["linux-x64"], &json!({ "version": "2.0.0" })), "linux");
    let (status, body) = publish_once(|token| {
        variants_only_request(&renamed("version", "2.0.0"), &[("linux-x64", &linux2)], token)
    })
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("names another version"), "{body}");

    let (_, other) =
        archive(&variant_manifest(&["linux-x64"], &json!({ "name": "@acme/other" })), "linux");
    let (status, body) = publish_once(|token| {
        variants_only_request(&any_manifest(), &[("linux-x64", &other)], token)
    })
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("name"), "{body}");
}

/// A version without an `any` archive is the version, not one of its
/// variants, so it declares no `variant`; and it names the package its URL
/// names even when the body leaves the package's name out.
#[tokio::test]
async fn a_version_without_an_any_archive_is_no_variant_of_another_package() {
    let (_, linux) = archive(&variant_manifest(&["linux-x64"], &json!({})), "linux");
    let mut marked = any_manifest();
    marked["variant"] = json!({ "platforms": ["linux-x64"] });
    let (status, body) =
        publish_once(|token| variants_only_request(&marked, &[("linux-x64", &linux)], token)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("declares a variant"), "{body}");

    let (_, nameless) = archive(&variant_manifest(&["linux-x64"], &json!({ "name": "" })), "linux");
    let (_tmp, app, token) = setup().await;
    let mut base = any_manifest();
    base["name"] = json!("");
    let (parts, body) =
        variants_only_request(&base, &[("linux-x64", &nameless)], &token).into_parts();
    let mut document: Value = serde_json::from_slice(&body_bytes(body).await).unwrap();
    document
        .as_object_mut()
        .unwrap()
        .remove("name");
    let request = Request::from_parts(parts, Body::from(serde_json::to_vec(&document).unwrap()));
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// The version's manifest is read as its archives' are, so empty peer maps
/// on both sides agree.
#[tokio::test]
async fn a_version_without_an_any_archive_compares_its_normalized_manifest() {
    let empty_peers = json!({ "peerDependencies": {}, "peerDependenciesMeta": {} });
    let mut base = any_manifest();
    base["peerDependencies"] = json!({});
    base["peerDependenciesMeta"] = json!({});
    let (_, linux) = archive(&variant_manifest(&["linux-x64"], &empty_peers), "linux");
    let (status, body) =
        publish_once(|token| variants_only_request(&base, &[("linux-x64", &linux)], token)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// A provenance statement may accompany a version published without an
/// `any` archive, and npm's documents link its attestations.
#[tokio::test]
async fn a_version_without_an_any_archive_keeps_its_provenance() {
    let (_tmp, app, token) = setup().await;
    let (_, linux) = archive(&variant_manifest(&["linux-x64"], &json!({})), "linux");
    let request = with_statement(
        variants_only_request(&any_manifest(), &[("linux-x64", &linux)], &token),
        &[("pkg:npm/%40acme/fast@1.0.0?variant=linux-x64", &linux)],
    )
    .await;
    let response = app
        .clone()
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let body = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let npm = body_json(get(&app, "/npm/~main/@acme%2Ffast", None).await.into_body()).await;
    let dist = &npm["versions"]["1.0.0"]["dist"];
    assert!(dist["attestations"]["url"].is_string(), "{dist}");
    assert!(dist["tarball"].is_string(), "{dist}");
}

/// A rewrite that removes another version keeps what pnpr derived for a
/// retained version without an `any` archive: its variants and the manifest
/// they share, which the rewritten document does not carry.
#[tokio::test]
async fn a_rewrite_keeps_the_variants_of_a_version_without_an_any_archive() {
    let (_tmp, app, token) = setup().await;
    for version in ["1.0.0", "1.1.0"] {
        let mut base = any_manifest();
        base["version"] = json!(version);
        let (_, linux) =
            archive(&variant_manifest(&["linux-x64"], &json!({ "version": version })), "linux");
        let response = app
            .clone()
            .oneshot(variants_only_request_at(version, &base, &[("linux-x64", &linux)], &token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }
    let mut packument =
        body_json(get(&app, "/npm/~main/@acme%2Ffast", None).await.into_body()).await;
    packument["versions"]
        .as_object_mut()
        .unwrap()
        .remove("1.1.0");
    packument["dist-tags"] = json!({ "latest": "1.0.0" });
    let rewrite =
        Request::put(format!("/npm/~acme/@acme/fast/-rev/{}", packument["_rev"].as_str().unwrap()))
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::from(serde_json::to_vec(&packument).unwrap()))
            .unwrap();
    let response = app
        .clone()
        .oneshot(rewrite)
        .await
        .unwrap();
    let status = response.status();
    let body = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let document = body_json(
        get(&app, "/npm/~main/@acme%2Ffast", Some("application/vnd.pnpr.install-v2+json"))
            .await
            .into_body(),
    )
    .await;
    let entry = &document["versions"]["1.0.0"];
    assert_eq!(entry["variants"][0]["tag"], "linux-x64", "{entry}");
    assert!(entry.get("manifest").is_some(), "{entry}");
}
