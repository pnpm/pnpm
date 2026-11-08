use super::{RegistryError, StatusCode, error_response, header};

#[tokio::test]
async fn open_circuit_response_preserves_retry_after_and_cargo_json() {
    let response = error_response(RegistryError::UpstreamUnavailable {
        upstream: "example-upstream".to_string(),
        retry_after: std::time::Duration::from_secs(12),
    });
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()[header::RETRY_AFTER], "12");
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        body["errors"][0]["detail"]
            .as_str()
            .unwrap()
            .contains("example-upstream"),
    );
}
