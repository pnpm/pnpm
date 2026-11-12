use super::{
    Body, Method, RegistryError, Request, ServiceExt, StatusCode, TempDir, app_with_token, header,
    record, signed, signed_json, to_bytes,
};

#[tokio::test]
async fn npm_authentication_errors_distinguish_missing_and_invalid_tokens() {
    let tmp = TempDir::new().unwrap();
    let app = app_with_token(&tmp, "valid-token", record(false, &[]));
    for path in ["/-/whoami", "/~local/-/whoami", "/npm/-/whoami", "/npm/~local/-/whoami"] {
        for (request, challenge, message) in [
            (
                Request::get(path)
                    .body(Body::empty())
                    .unwrap(),
                "Bearer",
                "Authentication required",
            ),
            (
                signed(Method::GET, path, "invalid-token"),
                r#"Bearer error="invalid_token""#,
                "Invalid authentication token",
            ),
            (
                signed(Method::GET, path, "pnpr_workload_not-a-jwt"),
                r#"Bearer error="invalid_token""#,
                "Invalid authentication token",
            ),
        ] {
            let response = app
                .clone()
                .oneshot(request)
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
            assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
            assert_eq!(response.headers()[header::WWW_AUTHENTICATE], challenge);
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["error"], body["reason"]);
            assert!(
                body["error"]
                    .as_str()
                    .unwrap()
                    .contains(message),
            );
        }
    }
}

#[tokio::test]
async fn npm_errors_keep_status_safe_messages_and_retry_headers() {
    use axum::{Router, middleware, response::IntoResponse, routing::get};
    use serde_json::json;
    use std::time::Duration;

    let app = Router::new()
        .route("/npm/missing", get(|| async { RegistryError::NotFound }))
        .route(
            "/npm/internal",
            get(|| async {
                RegistryError::Internal { reason: "private database detail".to_string() }
            }),
        )
        .route(
            "/npm/unpublished",
            get(|| async {
                RegistryError::CannotRepublishUnpublishedVersion {
                    package: "example-package".to_string(),
                    version: "1.0.0".to_string(),
                }
            }),
        )
        .route(
            "/npm/unavailable",
            get(|| async {
                RegistryError::UpstreamUnavailable {
                    upstream: "example".to_string(),
                    retry_after: Duration::from_millis(1500),
                }
                .into_response()
            }),
        )
        .layer(middleware::from_fn(super::super::routing::npm_errors::render));
    for (path, status, message) in [
        ("/npm/missing", StatusCode::NOT_FOUND, "Not Found"),
        ("/npm/internal", StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            json!({"error": message, "reason": message, "code": if status == StatusCode::NOT_FOUND { "ERR_PNPR_NOT_FOUND" } else { "ERR_PNPR_INTERNAL" }}),
        );
    }
    let response = app
        .clone()
        .oneshot(
            Request::get("/npm/unpublished")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("Cannot republish"),
    );
    assert_eq!(body["error"], body["reason"]);
    let response = app
        .oneshot(
            Request::get("/npm/unavailable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()[header::RETRY_AFTER], "2");
}

#[tokio::test]
async fn head_errors_have_no_body_and_owned_endpoints_return_json() {
    use axum::{Router, middleware, routing::get};
    let app = Router::new()
        .route("/npm/missing", get(|| async { RegistryError::NotFound }))
        .route("/-/pnpr/v0/missing", get(|| async { RegistryError::NotFound }))
        .layer(middleware::from_fn(super::super::routing::npm_errors::render));
    let response = app
        .clone()
        .oneshot(
            Request::head("/npm/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    assert!(to_bytes(response.into_body(), usize::MAX).await.unwrap().is_empty());
    let response = app
        .oneshot(
            Request::get("/-/pnpr/v0/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["code"],
        "ERR_PNPR_NOT_FOUND",
    );
}

#[tokio::test]
async fn oversized_npm_publish_explains_the_body_limit() {
    let tmp = TempDir::new().unwrap();
    let app = app_with_token(&tmp, "valid-token", record(false, &[]));
    let mut request = signed(Method::PUT, "/npm/example-package", "valid-token");
    *request.body_mut() = Body::from(vec![0; super::super::MAX_PUBLISH_BODY_BYTES + 1]);
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body["error"],
        "Publish request body exceeds the 100 MiB JSON limit; use staged archives for large tarballs",
    );
    assert_eq!(body["error"], body["reason"]);
}

#[tokio::test]
async fn malformed_json_request_bodies_return_bad_request() {
    let tmp = TempDir::new().unwrap();
    let app = app_with_token(&tmp, "valid-token", record(false, &[]));
    for (method, path, payload) in [
        (Method::PUT, "/-/user/org.couchdb.user:alice", "{not-json"),
        (Method::PUT, "/-/user/alice", "{not-json"),
        (Method::PUT, "/npm/-/user/org.couchdb.user:alice", "{not-json"),
        (Method::POST, "/-/npm/v1/tokens", "{not-json"),
        (Method::POST, "/-/npm/v1/tokens", r#"{"readonly":"yes"}"#),
        (Method::POST, "/npm/-/npm/v1/tokens", "{not-json"),
        (Method::POST, "/npm/-/stage/package/example-package", "{not-json"),
        (Method::PUT, "/npm/example-package", "{not-json"),
        (Method::PUT, "/npm/@scope%2fexample-package", "{not-json"),
        (Method::PUT, "/npm/example-package/-rev/1-0", "{not-json"),
    ] {
        let request = signed_json(method.clone(), path, "valid-token", payload);
        let response = app
            .clone()
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{method} {path} ({payload})");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json", "{path}");
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"], body["reason"], "{path}");
        let message = body["error"].as_str().unwrap();
        assert!(message.starts_with("Bad request: invalid request body:"), "{path}: {message}");
    }
}

#[tokio::test]
async fn native_publish_errors_have_the_same_shape_at_root_and_under_npm() {
    let tmp = TempDir::new().unwrap();
    let app = app_with_token(&tmp, "valid-token", record(false, &[]));
    for path in ["/-/pnpr/v0/publish", "/npm/-/pnpr/v0/publish"] {
        let response = app
            .clone()
            .oneshot(signed_json(Method::PUT, path, "valid-token", "{not-json"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json", "{path}");
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["code"], "ERR_PNPR_BAD_REQUEST", "{path}");
        assert_eq!(body["error"], body["reason"]);
    }
}

#[tokio::test]
async fn framework_rejections_on_native_endpoints_return_json() {
    use axum::{Router, middleware, routing::get};
    let app = Router::new()
        .route("/-/pnpr/v0/example", get(|| async { "ok" }))
        .layer(middleware::from_fn(super::super::routing::npm_errors::render));
    let response = app
        .oneshot(
            Request::post("/-/pnpr/v0/example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers()[header::ALLOW], "GET,HEAD");
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        body,
        serde_json::json!({
            "error":"Method Not Allowed", "reason":"Method Not Allowed", "code":"ERR_PNPR_METHOD_NOT_ALLOWED",
        }),
    );
}

#[tokio::test]
async fn publication_method_rejections_return_json_and_preserve_allowed_methods() {
    let tmp = TempDir::new().unwrap();
    let app = app_with_token(&tmp, "valid-token", record(false, &[]));
    for (path, allowed) in [
        ("/-/publications/example", "GET,HEAD"),
        ("/-/publications/example/missing", "GET,HEAD"),
        ("/-/publications/example/missing/notes", "GET,HEAD,PUT,DELETE"),
    ] {
        let response = app
            .clone()
            .oneshot(signed(Method::POST, path, "valid-token"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{path}");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json", "{path}");
        assert_eq!(response.headers()[header::ALLOW], allowed, "{path}");
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            serde_json::json!({
                "error": "Method Not Allowed",
                "reason": "Method Not Allowed",
                "code": "ERR_PNPR_METHOD_NOT_ALLOWED",
            }),
            "{path}",
        );
    }
}

#[tokio::test]
async fn publication_query_rejections_return_json_with_bodyless_head_errors() {
    let tmp = TempDir::new().unwrap();
    let app = app_with_token(&tmp, "valid-token", record(false, &[]));
    for method in [Method::GET, Method::HEAD] {
        let response = app
            .clone()
            .oneshot(signed(method.clone(), "/-/publications/example", "valid-token"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{method}");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json", "{method}");
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        if method == Method::HEAD {
            assert!(bytes.is_empty());
        } else {
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["code"], "ERR_PNPR_BAD_REQUEST");
            assert_eq!(body["error"], body["reason"]);
            assert!(
                body["reason"]
                    .as_str()
                    .unwrap()
                    .contains("member"),
            );
        }
    }
}
