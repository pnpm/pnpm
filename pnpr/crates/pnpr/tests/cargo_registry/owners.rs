use super::{
    AuthState, Body, Request, ServiceExt, StatusCode, TempDir, Value, body_bytes, cargo_config,
    header, json, router_with_auth,
};

#[tokio::test]
async fn owner_operations_return_a_cargo_error() {
    let tmp = TempDir::new().unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$authenticated"),
        AuthState::in_memory(),
    );
    for path in [
        "/cargo/api/v1/crates/demo/owners",
        "/cargo/api/v1/crates/missing-crate/owners",
        "/cargo/~main/api/v1/crates/demo/owners",
        "/cargo/~main/api/v1/crates/missing-crate/owners",
        "/cargo/~crates/api/v1/crates/demo/owners",
        "/cargo/~crates/api/v1/crates/missing-crate/owners",
    ] {
        for method in ["GET", "PUT", "DELETE"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(r#"{"users":["example-user"]}"#))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{method} {path}");
            assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
            let body: Value =
                serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
            assert_eq!(
                body,
                json!({"errors": [{"detail": "cargo owner is not supported by this registry"}]}),
            );
        }
    }
}
