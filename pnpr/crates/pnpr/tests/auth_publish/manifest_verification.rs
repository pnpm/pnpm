//! Publish-time manifest verification: the submitted version metadata must
//! agree with the archive's `package.json` under `npm-install-manifest/1`, and
//! the version is served from what the archive says.

use super::{
    Body, Request, ServiceExt, StatusCode, TempDir, Value, add_user_and_get_token, body_bytes,
    body_json, json, publish_doc, router, static_config,
};
use crate::npm::tgz_of;
use pnpr_storage::layout::local_document_path;

async fn publish(
    app: &axum::Router,
    token: &str,
    name: &str,
    doc: &Value,
) -> axum::response::Response {
    let request = Request::put(format!("/npm/{name}"))
        .header("content-type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(doc).unwrap()))
        .unwrap();
    app.clone()
        .oneshot(request)
        .await
        .unwrap()
}

async fn served_version(app: &axum::Router, name: &str, version: &str) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/npm/{name}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    body_json(response.into_body()).await["versions"][version].clone()
}

async fn setup() -> (TempDir, axum::Router, String) {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let (app, token) = add_user_and_get_token(app, "alice", "secret").await;
    (tmp, app, token)
}

fn manifest_tgz(manifest: &Value, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let manifest = manifest.to_string();
    let mut entries = vec![("package/package.json", manifest.as_bytes())];
    entries.extend_from_slice(extra);
    tgz_of(&entries)
}

#[tokio::test]
async fn a_dependency_the_submitted_metadata_misstates_is_rejected() {
    let (tmp, app, token) = setup().await;
    let archive = json!({
        "name": "widgets",
        "version": "1.2.0",
        "dependencies": { "core": "^1.0.0" },
        "scripts": { "postinstall": "node collect.js" },
    });
    let mut doc = publish_doc("widgets", "1.2.0", &manifest_tgz(&archive, &[]));
    let version = &mut doc["versions"]["1.2.0"];
    version["dependencies"] = json!({ "core": "^2.0.0" });
    version
        .as_object_mut()
        .unwrap()
        .remove("scripts");

    let response = publish(&app, &token, "widgets", &doc).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let reason = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(reason.contains("widgets@1.2.0"), "{reason}");
    assert!(reason.contains("npm-install-manifest/1"), "{reason}");
    assert!(reason.contains("dependencies.core"), "{reason}");
    assert!(reason.contains("scripts"), "{reason}");
    assert!(
        !local_document_path(tmp.path(), "", pnpr_storage::Ecosystem::Npm, "widgets").exists(),
        "a rejected publish must write nothing",
    );
}

#[tokio::test]
async fn a_script_only_the_archive_declares_is_rejected() {
    let (_tmp, app, token) = setup().await;
    let archive = json!({
        "name": "widgets",
        "version": "1.0.0",
        "scripts": { "preinstall": "node steal.js", "test": "node test.js" },
    });
    let mut doc = publish_doc("widgets", "1.0.0", &manifest_tgz(&archive, &[]));
    doc["versions"]["1.0.0"]["scripts"] = json!({ "test": "node test.js" });

    let response = publish(&app, &token, "widgets", &doc).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_archive_for_another_package_is_rejected() {
    let (_tmp, app, token) = setup().await;
    let archive = json!({ "name": "other", "version": "1.0.0" });
    let mut doc = publish_doc("widgets", "1.0.0", &manifest_tgz(&archive, &[]));
    // Even a submitted version that agrees with the archive lands in the
    // wrong package.
    doc["versions"]["1.0.0"]["name"] = json!("other");

    let response = publish(&app, &token, "widgets", &doc).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let reason = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(reason.contains("name"), "{reason}");
}

#[tokio::test]
async fn an_ambiguous_or_unsafe_archive_is_rejected() {
    let (_tmp, app, token) = setup().await;
    let manifest = br#"{"name":"widgets","version":"1.0.0"}"#;
    let archives = [
        tgz_of(&[("package/package.json", manifest), ("other/package.json", manifest)]),
        tgz_of(&[("package/package.json", manifest), ("package/PACKAGE.JSON", manifest)]),
        tgz_of(&[("package/index.js", b"")]),
    ];
    for tgz in archives {
        let doc = publish_doc("widgets", "1.0.0", &tgz);
        let response = publish(&app, &token, "widgets", &doc).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn a_tgz_that_does_not_decode_is_rejected() {
    let (_tmp, app, token) = setup().await;
    // A gzip header followed by bytes that do not inflate.
    let doc = publish_doc("widgets", "1.0.0", b"\x1f\x8b\x08\x00not deflate data");
    let response = publish(&app, &token, "widgets", &doc).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_npm_publish_document_is_accepted_and_served_from_the_archive() {
    let (_tmp, app, token) = setup().await;
    let archive = json!({
        "name": "widgets",
        "version": "1.0.0",
        "bin": "./bin/widgets.js",
        "optionalDependencies": { "native": "1.0.0" },
        "dependencies": {},
    });
    let tgz = manifest_tgz(&archive, &[("package/bin/widgets.js", b"#!/usr/bin/env node\n")]);
    let mut doc = publish_doc("widgets", "1.0.0", &tgz);
    // What npm's `prepare()` and npm 9's `normalize-package-data` submit for
    // the same `package.json`.
    let version = &mut doc["versions"]["1.0.0"];
    version["bin"] = json!({ "widgets": "bin/widgets.js" });
    version["dependencies"] = json!({ "native": "1.0.0" });
    version["engines"] = json!({ "node": "*" });
    version["description"] = json!("kept as submitted");

    let response = publish(&app, &token, "widgets", &doc).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(
        response
            .headers()
            .get("npm-notice")
            .is_none(),
    );

    let served = served_version(&app, "widgets", "1.0.0").await;
    assert_eq!(served["bin"], json!({ "widgets": "bin/widgets.js" }));
    assert!(served.get("dependencies").is_none(), "{served}");
    assert_eq!(served["optionalDependencies"], json!({ "native": "1.0.0" }));
    assert!(served.get("engines").is_none(), "{served}");
    assert_eq!(served["description"], "kept as submitted");
    assert_eq!(served["hasInstallScript"], false);
    assert_eq!(served["_hasShrinkwrap"], false);
    assert_eq!(served["_archiveManifest"]["profile"], "npm-install-manifest/1");
    let digest = served["_archiveManifest"]["packageJson"].as_str().unwrap();
    assert!(digest.starts_with("sha512-"), "{digest}");
    assert!(served["_archiveManifest"].get("npmShrinkwrap").is_none());
}

#[tokio::test]
async fn an_unbuildable_gyp_script_is_accepted_with_a_notice_and_not_served() {
    let (_tmp, app, token) = setup().await;
    let archive = json!({ "name": "widgets", "version": "2.3.3", "os": ["darwin"] });
    let mut doc = publish_doc("widgets", "2.3.3", &manifest_tgz(&archive, &[]));
    let version = &mut doc["versions"]["2.3.3"];
    version["gypfile"] = json!(true);
    version["scripts"] = json!({ "install": "node-gyp rebuild" });

    let response = publish(&app, &token, "widgets", &doc).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let notice = response
        .headers()
        .get("npm-notice")
        .expect("npm-notice header");
    assert!(
        notice
            .to_str()
            .unwrap()
            .contains("widgets@2.3.3"),
        "{notice:?}",
    );

    let served = served_version(&app, "widgets", "2.3.3").await;
    assert!(served.get("scripts").is_none(), "{served}");
    assert_eq!(served["hasInstallScript"], false);
}

#[tokio::test]
async fn a_gyp_script_that_differs_from_node_gyp_rebuild_is_rejected() {
    let (_tmp, app, token) = setup().await;
    let archive = json!({ "name": "widgets", "version": "1.0.0" });
    let mut doc = publish_doc("widgets", "1.0.0", &manifest_tgz(&archive, &[]));
    doc["versions"]["1.0.0"]["scripts"] = json!({ "install": "node-gyp rebuild && curl x" });

    let response = publish(&app, &token, "widgets", &doc).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_shrinkwrap_and_a_binding_gyp_are_derived_from_the_archive() {
    let (_tmp, app, token) = setup().await;
    let archive = json!({ "name": "widgets", "version": "1.0.0" });
    let tgz = manifest_tgz(
        &archive,
        &[("package/npm-shrinkwrap.json", b"{}"), ("package/binding.gyp", b"{}")],
    );
    let mut doc = publish_doc("widgets", "1.0.0", &tgz);
    // npm's `gypfile` step: the script an installer runs anyway.
    doc["versions"]["1.0.0"]["scripts"] = json!({ "install": "node-gyp rebuild" });

    let response = publish(&app, &token, "widgets", &doc).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let served = served_version(&app, "widgets", "1.0.0").await;
    assert_eq!(served["_hasShrinkwrap"], true);
    assert_eq!(served["hasInstallScript"], true);
    assert_eq!(served["scripts"]["install"], "node-gyp rebuild");
    let shrinkwrap = served["_archiveManifest"]["npmShrinkwrap"].as_str().unwrap();
    assert!(shrinkwrap.starts_with("sha512-"), "{shrinkwrap}");
}

#[tokio::test]
async fn a_publisher_supplied_archive_record_is_replaced() {
    let (_tmp, app, token) = setup().await;
    let archive = json!({ "name": "widgets", "version": "1.0.0" });
    let mut doc = publish_doc("widgets", "1.0.0", &manifest_tgz(&archive, &[]));
    doc["versions"]["1.0.0"]["_archiveManifest"] = json!({ "profile": "forged" });
    doc["versions"]["1.0.0"]["hasInstallScript"] = json!(true);

    let response = publish(&app, &token, "widgets", &doc).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let served = served_version(&app, "widgets", "1.0.0").await;
    assert_eq!(served["_archiveManifest"]["profile"], "npm-install-manifest/1");
    assert_eq!(served["hasInstallScript"], false);
}
