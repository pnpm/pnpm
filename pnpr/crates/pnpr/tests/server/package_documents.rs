//! pnpr's own package document and release records, as a client that asks
//! for `application/vnd.pnpr.install-v2+json` reads them.

use super::{
    AuthState, Body, Request, ServiceExt, StatusCode, TempDir, body_json, config_for,
    foo_packument, header, json, router, router_with_auth, sha512_integrity,
};
use crate::tokens::issue_token;
use axum::response::Response;

const INSTALL_V2: &str = "application/vnd.pnpr.install-v2+json";

async fn get(app: &axum::Router, uri: &str, accept: Option<&str>, token: Option<&str>) -> Response {
    let mut request = Request::get(uri);
    if let Some(accept) = accept {
        request = request.header(header::ACCEPT, accept);
    }
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

fn header_of<'res>(response: &'res Response, name: impl header::AsHeaderName) -> Option<&'res str> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
}

async fn upstream_with_foo() -> mockito::ServerGuard {
    let mut upstream = mockito::Server::new_async().await;
    let mut packument = foo_packument(&upstream.url());
    packument["time"] = json!({ "1.0.0": "2026-01-01T00:00:00.000Z" });
    packument["versions"]["1.0.0"]["dependencies"] = json!({ "bar": "^1.0.0" });
    upstream
        .mock("GET", "/foo")
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .create_async()
        .await;
    upstream
}

#[tokio::test]
async fn the_document_is_negotiated_by_accept_and_identifies_pnpr() {
    let upstream = upstream_with_foo().await;
    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response = get(&app, "/npm/foo", Some(INSTALL_V2), None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_of(&response, header::CONTENT_TYPE), Some(INSTALL_V2));
    assert_eq!(header_of(&response, header::VARY), Some("Accept"));
    assert_eq!(header_of(&response, "pnpr-registry"), Some("1"));
    let document = body_json(response.into_body()).await;
    assert_eq!(document["distTags"], json!({ "latest": "1.0.0" }));
    assert_eq!(document["timesSince"], json!("2025-12-02T00:00:00.000Z"));
    let entry = &document["versions"]["1.0.0"];
    assert_eq!(entry["time"], json!("2026-01-01T00:00Z"));
    assert_eq!(entry["integrity"], json!(sha512_integrity(b"foo tarball")));
    assert_eq!(entry["manifest"], json!({ "dependencies": { "bar": "^1.0.0" } }));

    let abbreviated =
        get(&app, "/npm/foo", Some("application/vnd.npm.install-v1+json"), None).await;
    assert_eq!(header_of(&abbreviated, "pnpr-registry"), Some("1"));
    assert_ne!(header_of(&abbreviated, header::CONTENT_TYPE), Some(INSTALL_V2));
}

#[tokio::test]
async fn the_document_does_not_change_as_time_passes() {
    let upstream = upstream_with_foo().await;
    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));
    let first = get(&app, "/npm/foo", Some(INSTALL_V2), None).await;
    let etag = header_of(&first, header::ETAG).unwrap().to_string();
    // The publish time is far older than the precision horizon of npm's
    // abbreviated document, yet keeps its minute: it is coarsened against
    // the package's last publish, not the current time.
    let document = body_json(first.into_body()).await;
    assert_eq!(document["versions"]["1.0.0"]["time"], json!("2026-01-01T00:00Z"));
    let second = get(&app, "/npm/foo", Some(INSTALL_V2), None).await;
    assert_eq!(header_of(&second, header::ETAG), Some(etag.as_str()));
}

#[tokio::test]
async fn a_release_record_is_immutable_and_names_its_version() {
    let upstream = upstream_with_foo().await;
    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response = get(&app, "/npm/foo/1.0.0+r0", None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header_of(&response, header::CACHE_CONTROL),
        Some("public, max-age=31536000, immutable"),
    );
    let record = body_json(response.into_body()).await;
    assert_eq!(
        record,
        json!({
            "name": "foo",
            "version": "1.0.0",
            "integrity": sha512_integrity(b"foo tarball"),
            "manifest": { "dependencies": { "bar": "^1.0.0" } },
        }),
    );

    // A version outside the revision protocol has only its original.
    let missing = get(&app, "/npm/foo/1.0.0+r1", None, None).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    // The version document without a revision stays npm's view of the
    // selected revision, with its usual caching.
    let selected = get(&app, "/npm/foo/1.0.0", None, None).await;
    assert_ne!(
        header_of(&selected, header::CACHE_CONTROL),
        Some("public, max-age=31536000, immutable"),
    );
    assert_eq!(body_json(selected.into_body()).await["version"], json!("1.0.0"));
}

#[tokio::test]
async fn caller_gated_documents_and_records_stay_out_of_shared_caches() {
    let upstream = upstream_with_foo().await;
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(config_for(&upstream.url(), tmp.path().to_path_buf()), auth);

    for (uri, accept) in [("/npm/~npmjs/foo", Some(INSTALL_V2)), ("/npm/~npmjs/foo/1.0.0+r0", None)]
    {
        let response = get(&app, uri, accept, Some(&token)).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        assert_eq!(header_of(&response, header::CACHE_CONTROL), Some("private, no-store"), "{uri}");
        let vary = header_of(&response, header::VARY).unwrap_or_default();
        assert!(vary.contains("Authorization"), "{uri}: {vary}");
    }
}
