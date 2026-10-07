use super::{body_display_string, read_sanitized_error_body};
use pnpm_network::LimitedBody;

#[test]
fn body_display_string_sanitizes_controls() {
    let body = LimitedBody { bytes: b"hello\x00\x08world".to_vec(), truncated: false };
    assert_eq!(body_display_string(&body), "helloworld");
}

#[test]
fn body_display_string_appends_truncated_notice() {
    let body = LimitedBody { bytes: b"error message".to_vec(), truncated: true };
    assert_eq!(body_display_string(&body), "error message (response body truncated)");
}

#[tokio::test]
async fn read_sanitized_error_body_handles_response() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/err")
        .with_status(404)
        .with_body("Package not found\x07")
        .create_async()
        .await;

    let response = reqwest::get(format!("{}/err", server.url())).await.unwrap();
    let (status, status_text, body) = read_sanitized_error_body(response, 1024).await;
    assert_eq!(status, reqwest::StatusCode::NOT_FOUND);
    assert_eq!(status_text, "Not Found");
    assert_eq!(body, "Package not found");
}
