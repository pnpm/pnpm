use super::*;

#[tokio::test]
async fn npm_authentication_errors_distinguish_missing_and_invalid_tokens() {
    let tmp = TempDir::new().unwrap();
    let app = app_with_token(&tmp, "valid-token", record(false, &[]));
    for path in ["/-/whoami", "/~local/-/whoami", "/npm/-/whoami", "/npm/~local/-/whoami"] {
        for token in [None, Some("invalid-token")] {
            let request = match token {
                Some(token) => signed(Method::GET, path, token),
                None => Request::get(path)
                    .body(Body::empty())
                    .unwrap(),
            };
            let response = app
                .clone()
                .oneshot(request)
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
            assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
            let expected_challenge =
                if token.is_some() { "Bearer error=\"invalid_token\"" } else { "Bearer" };
            assert_eq!(response.headers()[header::WWW_AUTHENTICATE], expected_challenge);
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["error"], body["reason"]);
            if token.is_some() {
                assert_eq!(body["error"], "Invalid authentication token");
            } else {
                assert!(
                    body["error"]
                        .as_str()
                        .unwrap()
                        .contains("Authentication required")
                );
            }
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
            json!({"error": message, "reason": message})
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
            .contains("Cannot republish")
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
async fn npm_head_errors_have_no_body_and_other_protocols_keep_plain_text() {
    use axum::{Router, middleware, routing::get};
    let app = Router::new()
        .route("/npm/missing", get(|| async { RegistryError::NotFound }))
        .route("/cargo/missing", get(|| async { RegistryError::NotFound }))
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
            Request::get("/cargo/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "text/plain; charset=utf-8");
    assert_eq!(to_bytes(response.into_body(), usize::MAX).await.unwrap().as_ref(), b"Not Found");
}
