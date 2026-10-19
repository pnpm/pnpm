use super::{
    AuthState, Body, Duration, Request, ServiceExt, StatusCode, TempDir, body_bytes, config_for,
    foo_packument, header, json, router, router_with_auth,
};
use crate::tokens::issue_token;
use pnpr_storage::layout::local_document_path;

#[tokio::test]
async fn public_packument_responses_carry_caching_headers() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = foo_packument(&upstream.url());
    let _mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.http.packument_ttl = Duration::from_mins(5);
    let app = router(config);

    // Full packument GET
    let response = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|header_value| header_value.to_str().ok()),
        Some("public, max-age=300"),
    );
    assert_eq!(
        response
            .headers()
            .get(header::VARY)
            .and_then(|header_value| header_value.to_str().ok()),
        Some("Accept"),
    );
    let full_etag = response
        .headers()
        .get(header::ETAG)
        .and_then(|header_value| header_value.to_str().ok())
        .expect("ETag header must be present")
        .to_string();
    assert!(full_etag.starts_with('"') && full_etag.ends_with('"'));

    // Abbreviated packument GET
    let abbrev_response = app
        .oneshot(
            Request::get("/npm/foo")
                .header(header::ACCEPT, "application/vnd.npm.install-v1+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(abbrev_response.status(), StatusCode::OK);
    assert_eq!(
        abbrev_response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|header_value| header_value.to_str().ok()),
        Some("public, max-age=300"),
    );
    assert_eq!(
        abbrev_response
            .headers()
            .get(header::VARY)
            .and_then(|header_value| header_value.to_str().ok()),
        Some("Accept"),
    );
    let abbrev_etag = abbrev_response
        .headers()
        .get(header::ETAG)
        .and_then(|header_value| header_value.to_str().ok())
        .expect("ETag header must be present")
        .to_string();
    assert_ne!(full_etag, abbrev_etag, "abbreviated packument has distinct body and ETag");
}

#[tokio::test]
async fn packument_conditional_if_none_match() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = foo_packument(&upstream.url());
    let _mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.http.packument_ttl = Duration::from_mins(5);
    let app = router(config);

    let initial = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(initial.status(), StatusCode::OK);
    let etag = initial
        .headers()
        .get(header::ETAG)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // Matching strong ETag
    let not_modified = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .header(header::IF_NONE_MATCH, &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        not_modified
            .headers()
            .get(header::ETAG)
            .and_then(|header_value| header_value.to_str().ok()),
        Some(etag.as_str()),
    );
    assert_eq!(
        not_modified
            .headers()
            .get(header::VARY)
            .and_then(|header_value| header_value.to_str().ok()),
        Some("Accept"),
    );
    assert_eq!(
        not_modified
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|header_value| header_value.to_str().ok()),
        Some("public, max-age=300"),
    );
    assert_eq!(body_bytes(not_modified.into_body()).await.len(), 0);

    // Matching weak ETag
    let weak_match = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .header(header::IF_NONE_MATCH, format!("W/{etag}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(weak_match.status(), StatusCode::NOT_MODIFIED);

    // Wildcard *
    let wildcard = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .header(header::IF_NONE_MATCH, "*")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wildcard.status(), StatusCode::NOT_MODIFIED);

    // Comma-separated list containing matching ETag
    let list_match = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .header(header::IF_NONE_MATCH, format!(r#""other", {etag}"#))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_match.status(), StatusCode::NOT_MODIFIED);

    // Multiple If-None-Match headers where a later one matches returns 304
    let mut multi_req = Request::get("/npm/foo")
        .body(Body::empty())
        .unwrap();
    multi_req
        .headers_mut()
        .append(header::IF_NONE_MATCH, header::HeaderValue::from_static(r#""non-matching-etag""#));
    multi_req
        .headers_mut()
        .append(header::IF_NONE_MATCH, header::HeaderValue::from_str(&etag).unwrap());
    let multi_match = app
        .clone()
        .oneshot(multi_req)
        .await
        .unwrap();
    assert_eq!(multi_match.status(), StatusCode::NOT_MODIFIED);

    // Non-matching ETag returns 200 OK with body
    let mismatch = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .header(header::IF_NONE_MATCH, r#""non-matching-etag""#)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(mismatch.status(), StatusCode::OK);
    assert!(!body_bytes(mismatch.into_body()).await.is_empty());

    // Unquoted candidate does not match quoted ETag
    let unquoted = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .header(header::IF_NONE_MATCH, etag.trim_matches('"'))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unquoted.status(), StatusCode::OK);

    // Quoted candidate with embedded comma is not split into fragments
    let comma_etag = app
        .oneshot(
            Request::get("/npm/foo")
                .header(header::IF_NONE_MATCH, format!(r#""{},other""#, etag.trim_matches('"')))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(comma_etag.status(), StatusCode::OK);
}

#[tokio::test]
async fn packument_conditional_if_modified_since() {
    let mut upstream = mockito::Server::new_async().await;
    let mut packument = foo_packument(&upstream.url());
    packument["time"] = json!({ "modified": "2026-06-21T12:00:00Z" });
    let _mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.http.packument_ttl = Duration::from_mins(5);
    let app = router(config);

    let initial = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(initial.status(), StatusCode::OK);
    let last_modified = initial
        .headers()
        .get(header::LAST_MODIFIED)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // Exact Last-Modified in If-Modified-Since returns 304
    let ims_exact = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .header(header::IF_MODIFIED_SINCE, &last_modified)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ims_exact.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        ims_exact
            .headers()
            .get(header::LAST_MODIFIED)
            .and_then(|header_value| header_value.to_str().ok()),
        Some(last_modified.as_str()),
    );
    assert_eq!(body_bytes(ims_exact.into_body()).await.len(), 0);

    // Later date in If-Modified-Since returns 304
    let ims_later = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .header(header::IF_MODIFIED_SINCE, "Mon, 22 Jun 2026 12:00:00 GMT")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ims_later.status(), StatusCode::NOT_MODIFIED);

    // Earlier date in If-Modified-Since returns 200 OK
    let ims_earlier = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .header(header::IF_MODIFIED_SINCE, "Sat, 20 Jun 2026 12:00:00 GMT")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ims_earlier.status(), StatusCode::OK);
    assert!(!body_bytes(ims_earlier.into_body()).await.is_empty());

    // If-None-Match takes precedence over If-Modified-Since per RFC 9110
    let inm_mismatch_ims_match = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .header(header::IF_NONE_MATCH, r#""stale-etag""#)
                .header(header::IF_MODIFIED_SINCE, &last_modified)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(inm_mismatch_ims_match.status(), StatusCode::OK);

    // Non-ASCII If-None-Match still takes precedence and ignores If-Modified-Since
    let non_ascii_inm = app
        .oneshot(
            Request::get("/npm/foo")
                .header(
                    header::IF_NONE_MATCH,
                    header::HeaderValue::from_bytes(&[0x80, 0x81]).unwrap(),
                )
                .header(header::IF_MODIFIED_SINCE, &last_modified)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(non_ascii_inm.status(), StatusCode::OK);
}

#[tokio::test]
async fn private_and_named_registry_packuments_scoped_cache_control_and_vary() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = foo_packument(&upstream.url());
    let _mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let config = config_for(&upstream.url(), tmp.path().to_path_buf());
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(config, auth);

    // Named registry: /~npmjs/foo is always caller-scoped
    let response = app
        .clone()
        .oneshot(
            Request::get("/npm/~npmjs/foo")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|header_value| header_value.to_str().ok()),
        Some("private, no-store"),
    );
    assert_eq!(
        response
            .headers()
            .get(header::VARY)
            .and_then(|header_value| header_value.to_str().ok()),
        Some("Accept, Authorization"),
    );
    let etag = response
        .headers()
        .get(header::ETAG)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // Conditional 304 on named registry preserves private, no-store and Accept, Authorization
    let not_modified = app
        .oneshot(
            Request::get("/npm/~npmjs/foo")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::IF_NONE_MATCH, &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        not_modified
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|header_value| header_value.to_str().ok()),
        Some("private, no-store"),
    );
    assert_eq!(
        not_modified
            .headers()
            .get(header::VARY)
            .and_then(|header_value| header_value.to_str().ok()),
        Some("Accept, Authorization"),
    );
}

#[tokio::test]
async fn per_upstream_maxage_overrides_global_ttl_in_cache_control() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = foo_packument(&upstream.url());
    let _mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.http.packument_ttl = Duration::from_mins(1);
    config.routing.upstreams.get_mut("npmjs").unwrap().maxage = Some(Duration::from_mins(15));
    let app = router(config);

    let response = app
        .oneshot(
            Request::get("/npm/foo")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|header_value| header_value.to_str().ok()),
        Some("public, max-age=900"),
    );
}

#[tokio::test]
async fn hosted_packument_responses_use_global_packument_ttl_in_cache_control() {
    let tmp = TempDir::new().unwrap();
    let document = local_document_path(tmp.path(), "acme", "@acme/widget");
    std::fs::create_dir_all(document.parent().unwrap()).unwrap();
    let doc = json!({
        "name": "@acme/widget",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "@acme/widget",
                "version": "1.0.0",
                "dist": {
                    "tarball": "http://example.test/@acme/widget/-/widget-1.0.0.tgz",
                }
            }
        }
    });
    std::fs::write(&document, doc.to_string()).unwrap();

    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.http.packument_ttl = Duration::from_mins(2);
    config.routing.hosted.insert("acme".to_string(), super::hosted_with_access("acme", "$all"));
    config.routing.registries = super::Registries::new(
        vec![("acme".to_string(), super::Registry::Hosted { patterns: vec![] })]
            .into_iter()
            .collect(),
        Some("acme".to_string()),
    );
    let app = router_with_auth(config, AuthState::in_memory());

    let response = app
        .oneshot(
            Request::get("/npm/@acme/widget")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|header_value| header_value.to_str().ok()),
        Some("public, max-age=120"),
    );
    assert_eq!(
        response
            .headers()
            .get(header::VARY)
            .and_then(|header_value| header_value.to_str().ok()),
        Some("Accept"),
    );
    let etag = response
        .headers()
        .get(header::ETAG)
        .and_then(|header_value| header_value.to_str().ok())
        .unwrap();
    assert!(etag.starts_with('"') && etag.ends_with('"'));
}

#[tokio::test]
async fn anonymous_only_package_carries_private_cache_headers_and_denies_authenticated_caller() {
    let tmp = TempDir::new().unwrap();
    let document = local_document_path(tmp.path(), "acme", "@acme/widget");
    std::fs::create_dir_all(document.parent().unwrap()).unwrap();
    let doc = json!({
        "name": "@acme/widget",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "@acme/widget",
                "version": "1.0.0",
                "dist": {
                    "tarball": "http://example.test/@acme/widget/-/widget-1.0.0.tgz",
                }
            }
        }
    });
    std::fs::write(&document, doc.to_string()).unwrap();

    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.routing.hosted.insert(
        "acme".to_string(),
        super::hosted_with_access("acme", "$anonymous"),
    );
    config.routing.registries = super::Registries::new(
        vec![("acme".to_string(), super::Registry::Hosted { patterns: vec![] })]
            .into_iter()
            .collect(),
        Some("acme".to_string()),
    );
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(config, auth);

    let response = app
        .clone()
        .oneshot(
            Request::get("/npm/@acme/widget")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|header_value| header_value.to_str().ok()),
        Some("private, no-store"),
    );
    assert_eq!(
        response
            .headers()
            .get(header::VARY)
            .and_then(|header_value| header_value.to_str().ok()),
        Some("Accept, Authorization"),
    );

    let auth_response = app
        .oneshot(
            Request::get("/npm/@acme/widget")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(auth_response.status(), StatusCode::OK);
}

#[tokio::test]
async fn transformed_abbreviated_packument_skips_date_validation_but_honors_if_none_match() {
    let mut upstream = mockito::Server::new_async().await;
    let mut packument = foo_packument(&upstream.url());
    packument["time"] = json!({ "modified": "2026-06-21T12:00:00Z" });
    let _mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.http.packument_ttl = Duration::from_mins(5);
    let app = router(config);

    let initial = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .header(header::ACCEPT, "application/vnd.npm.install-v1+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(initial.status(), StatusCode::OK);
    assert!(
        initial
            .headers()
            .get(header::LAST_MODIFIED)
            .is_none(),
    );
    let etag = initial
        .headers()
        .get(header::ETAG)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let ims = app
        .clone()
        .oneshot(
            Request::get("/npm/foo")
                .header(header::ACCEPT, "application/vnd.npm.install-v1+json")
                .header(header::IF_MODIFIED_SINCE, "Sun, 21 Jun 2026 12:00:00 GMT")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ims.status(), StatusCode::OK);
    assert!(!body_bytes(ims.into_body()).await.is_empty());

    let inm = app
        .oneshot(
            Request::get("/npm/foo")
                .header(header::ACCEPT, "application/vnd.npm.install-v1+json")
                .header(header::IF_NONE_MATCH, &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(inm.status(), StatusCode::NOT_MODIFIED);
    assert!(
        inm.headers()
            .get(header::LAST_MODIFIED)
            .is_none(),
    );
    assert_eq!(body_bytes(inm.into_body()).await.len(), 0);
}
