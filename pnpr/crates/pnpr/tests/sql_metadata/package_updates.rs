use super::{
    packument,
    request,
    send,
    sqlite_config,
    token,
};
use crate::npm::publish_doc;
use axum::{
    body::to_bytes,
    http::{
        StatusCode,
        header,
    },
};
use pnpr::router;
use serde_json::{
    Value,
    json,
};
use tempfile::TempDir;
use tower::ServiceExt;

#[tokio::test]
async fn write_reads_and_deprecations_use_the_current_revision() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    let path = "/npm/sql-pkg";
    let publish = publish_doc("sql-pkg", "1.0.0", b"version");
    assert_eq!(
        send(&app, request("PUT", path, &token, Some(&publish))).await.0,
        StatusCode::CREATED,
    );

    let mut get = request("GET", "/npm/sql-pkg?write=true", &token, None);
    get.headers_mut()
        .insert(header::ACCEPT, "application/vnd.npm.install-v1+json".parse().unwrap());
    let response = app.clone().oneshot(get).await.unwrap();
    assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let mut document: Value = serde_json::from_slice(&body).unwrap();
    assert!(document["_rev"].is_string());
    document["versions"]["1.0.0"]["deprecated"] = json!("use the next version");
    assert_eq!(
        send(&app, request("PUT", path, &token, Some(&document))).await.0,
        StatusCode::CREATED,
    );

    let (_, mut update) = packument(&app, &token).await;
    update["versions"]["1.0.0"]["deprecated"] = json!([]);
    assert_eq!(
        send(&app, request("PUT", path, &token, Some(&update))).await.0,
        StatusCode::BAD_REQUEST,
    );
    update["versions"]["1.0.0"]["deprecated"] = json!("");
    assert_eq!(
        send(&app, request("PUT", path, &token, Some(&update))).await.0,
        StatusCode::CREATED,
    );
    assert!(packument(&app, &token).await.1["versions"]["1.0.0"].get("deprecated").is_none());
    assert_eq!(
        send(&app, request("PUT", path, &token, Some(&document))).await.0,
        StatusCode::CONFLICT,
    );
}

#[tokio::test]
async fn latest_moves_replace_the_package_readme() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    for version in ["1.0.0", "2.0.0", "3.0.0-beta.1"] {
        let mut doc = publish_doc("sql-pkg", version, version.as_bytes());
        if version == "3.0.0-beta.1" {
            doc["dist-tags"] = json!({"beta": version});
        } else if version == "2.0.0" {
            doc["readme"] = json!(version);
            doc["readmeFilename"] = json!(format!("{version}.md"));
        } else {
            doc["versions"][version]["readme"] = json!(version);
            doc["versions"][version]["readmeFilename"] = json!(format!("{version}.md"));
        }
        assert_eq!(
            send(&app, request("PUT", "/npm/sql-pkg", &token, Some(&doc))).await.0,
            StatusCode::CREATED,
        );
    }
    assert_eq!(packument(&app, &token).await.1["readme"], "2.0.0");
    for version in ["3.0.0-beta.1", "2.0.0"] {
        let tag = "/npm/-/package/sql-pkg/dist-tags/latest";
        assert_eq!(
            send(&app, request("PUT", tag, &token, Some(&json!(version)))).await.0,
            StatusCode::CREATED,
        );
        let (_, document) = packument(&app, &token).await;
        for key in ["readme", "readmeFilename"] {
            assert_eq!(document.get(key), document["versions"][version].get(key));
        }
    }
    let path = "/npm/sql-pkg/-/sql-pkg-2.0.0.tgz";
    assert!(send(&app, request("DELETE", path, &token, None)).await.0.is_success());
    let (_, document) = packument(&app, &token).await;
    assert_eq!(document["dist-tags"]["latest"], "1.0.0");
    assert_eq!(document["readme"], "1.0.0");
    assert_eq!(document["readmeFilename"], "1.0.0.md");

    let path = "/npm/sql-pkg/-/sql-pkg-1.0.0.tgz";
    assert!(send(&app, request("DELETE", path, &token, None)).await.0.is_success());
    let (_, document) = packument(&app, &token).await;
    assert_eq!(document["dist-tags"]["latest"], "3.0.0-beta.1");
    assert!(document.get("readme").is_none());
    assert!(document.get("readmeFilename").is_none());
}

#[tokio::test]
async fn a_publish_requires_versions_before_it_can_claim_a_name() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    for body in [json!({"name": "sql-pkg"}), json!({"name": "sql-pkg", "versions": {}})] {
        for (method, path) in [("PUT", "/npm/sql-pkg"), ("POST", "/npm/-/stage/package/sql-pkg")] {
            assert_eq!(
                send(&app, request(method, path, &token, Some(&body))).await.0,
                StatusCode::BAD_REQUEST,
            );
        }
    }
    assert_eq!(
        send(&app, request("GET", "/npm/sql-pkg", &token, None)).await.0,
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn write_reads_preserve_advisory_filtered_versions_during_partial_unpublish() {
    let tmp = TempDir::new().unwrap();
    let mut config = sqlite_config(tmp.path());
    let osv = crate::server::osv_database("sql-pkg", &["1.0.0"]);
    crate::server::enable_osv(&mut config, osv.path());
    let app = router(config);
    let token = token(&app).await;
    for version in ["1.0.0", "1.1.0", "1.2.0"] {
        let doc = publish_doc("sql-pkg", version, version.as_bytes());
        assert_eq!(
            send(&app, request("PUT", "/npm/sql-pkg", &token, Some(&doc))).await.0,
            StatusCode::CREATED,
        );
    }
    assert!(packument(&app, &token).await.1["versions"].get("1.0.0").is_none());
    let path = "/npm/sql-pkg?write=true";
    let anonymous = axum::http::Request::get(path)
        .body(axum::body::Body::empty())
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(anonymous)
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED,
    );
    let (status, _, bytes) = send(&app, request("GET", path, &token, None)).await;
    assert_eq!(status, StatusCode::OK);
    let mut doc: Value = serde_json::from_slice(&bytes).unwrap();
    let revision = format!("/npm/sql-pkg/-rev/{}", doc["_rev"].as_str().unwrap());
    assert!(doc["versions"]["1.0.0"].is_object());
    doc["versions"]
        .as_object_mut()
        .unwrap()
        .remove("1.1.0");
    assert_eq!(
        send(&app, request("PUT", &revision, &token, Some(&doc))).await.0,
        StatusCode::CREATED,
    );
    let (_, _, bytes) = send(&app, request("GET", path, &token, None)).await;
    let doc: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(doc["versions"]["1.0.0"].is_object());
    assert!(doc["versions"].get("1.1.0").is_none());
    assert!(packument(&app, &token).await.1["versions"].get("1.0.0").is_none());
}
