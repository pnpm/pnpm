use super::{
    AuthState, Body, Request, ServiceExt, StatusCode, TempDir, body_bytes, cargo_config,
    crate_archive, header, metadata, publish_body, publish_request, router_with_auth, sha256_hex,
};
use crate::tokens::issue_token;
use axum::{Router, response::Response};

async fn index(app: &Router, path: &str, validators: &[String], token: Option<&str>) -> Response {
    let mut request = Request::get(path);
    for validator in validators {
        request = request.header(header::IF_NONE_MATCH, validator);
    }
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, token);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn assert_revalidation(app: &Router, path: &str, token: Option<&str>) -> String {
    let response = index(app, path, &[], token).await;
    assert_eq!(response.status(), StatusCode::OK);
    let etag = response.headers()[header::ETAG]
        .to_str()
        .unwrap()
        .to_string();
    let body = body_bytes(response.into_body()).await;
    assert_eq!(etag, format!(r#""{}""#, sha256_hex(&body)));
    for validators in [
        vec![etag.clone()],
        vec![format!("W/{etag}")],
        vec!["*".to_string()],
        vec![format!(r#""other", {etag}"#)],
        vec![r#""other""#.to_string(), etag.clone()],
    ] {
        let response = index(app, path, &validators, token).await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED, "{validators:?}");
        assert_eq!(response.headers()[header::ETAG], etag);
        if token.is_some() {
            assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");
            assert_eq!(response.headers()[header::VARY], "Authorization");
        }
        assert!(body_bytes(response.into_body()).await.is_empty());
    }
    for validator in [r#""stale""#.to_string(), etag.trim_matches('"').to_string()] {
        let response = index(app, path, &[validator], token).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response.into_body()).await, body);
    }
    etag
}

#[tokio::test]
async fn hosted_index_revalidates_and_changes_on_publish_and_yank() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "publisher").await;
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all"),
        auth,
    );
    let path = "/cargo/index/de/mo/demo";
    let mut previous = String::new();
    for version in ["0.1.0", "0.2.0"] {
        let response = app
            .clone()
            .oneshot(publish_request(
                Some(&token),
                publish_body(&metadata("demo", version), &crate_archive("demo", version)),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = index(&app, path, std::slice::from_ref(&previous), None).await;
        assert_eq!(response.status(), StatusCode::OK);
        let current = assert_revalidation(&app, path, None).await;
        assert_ne!(current, previous);
        previous = current;
    }
    let response = app
        .clone()
        .oneshot(
            Request::delete("/cargo/api/v1/crates/demo/0.2.0/yank")
                .header(header::AUTHORIZATION, &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = index(&app, path, std::slice::from_ref(&previous), None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_ne!(response.headers()[header::ETAG], previous);
    let body = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(body.contains(r#""yanked":true"#));
    assert_revalidation(&app, "/cargo/~crates/index/de/mo/demo", None).await;
    let missing = index(&app, "/cargo/index/in/fl/inflector", &["*".to_string()], None).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert!(!missing.headers().contains_key(header::ETAG));
}

#[tokio::test]
async fn private_index_checks_access_before_revalidation() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "publisher").await;
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$authenticated"),
        auth,
    );
    let response = app
        .clone()
        .oneshot(publish_request(
            Some(&token),
            publish_body(&metadata("demo", "0.1.0"), &crate_archive("demo", "0.1.0")),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let path = "/cargo/index/de/mo/demo";
    let etag = assert_revalidation(&app, path, Some(&token)).await;
    for validator in [etag, "*".to_string()] {
        let denied = index(&app, path, &[validator], None).await;
        assert_eq!(denied.status(), StatusCode::NOT_FOUND);
        assert!(!denied.headers().contains_key(header::ETAG));
    }
}

#[tokio::test]
async fn proxied_index_revalidates_cached_bytes_without_an_upstream_etag() {
    let mut upstream = mockito::Server::new_async().await;
    let body = "{\"name\":\"sample\",\"vers\":\"1.0.0\"}\n";
    let mock = upstream
        .mock("GET", "/sa/mp/sample")
        .with_body(body)
        .expect(1)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), &upstream.url(), "$all"),
        AuthState::in_memory(),
    );
    assert_revalidation(&app, "/cargo/index/sa/mp/sample", None).await;
    mock.assert_async().await;
}

#[tokio::test]
async fn refreshed_upstream_index_does_not_match_the_old_etag() {
    let mut upstream = mockito::Server::new_async().await;
    let old = upstream
        .mock("GET", "/sa/mp/sample")
        .with_body("old\n")
        .expect(1)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let mut config = cargo_config(tmp.path().to_path_buf(), &upstream.url(), "$all");
    config.http.packument_ttl = std::time::Duration::ZERO;
    let app = router_with_auth(config, AuthState::in_memory());
    let path = "/cargo/index/sa/mp/sample";
    let response = index(&app, path, &[], None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let etag = response.headers()[header::ETAG]
        .to_str()
        .unwrap()
        .to_string();
    old.assert_async().await;
    old.remove_async().await;
    let updated = upstream
        .mock("GET", "/sa/mp/sample")
        .with_body("updated\n")
        .expect(1)
        .create_async()
        .await;
    let response = index(&app, path, std::slice::from_ref(&etag), None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_ne!(response.headers()[header::ETAG], etag);
    assert_eq!(body_bytes(response.into_body()).await, b"updated\n");
    updated.assert_async().await;
}
