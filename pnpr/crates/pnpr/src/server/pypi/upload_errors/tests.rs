use super::response;
use axum::{Router, body::to_bytes, routing::get};
use hyper::ext::ReasonPhrase;
use pnpr_error::RegistryError;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn upload_failure_reason_is_written_on_the_http1_status_line() {
    let app = Router::new()
        .route(
            "/",
            get(async || {
                response(RegistryError::BadRequest { reason: "file already exists".to_string() })
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut bytes = Vec::new();
    let received =
        tokio::time::timeout(Duration::from_secs(5), client.read_to_end(&mut bytes)).await;
    server.abort();
    received.unwrap().unwrap();
    assert!(bytes.starts_with(b"HTTP/1.1 400 Bad request: file already exists\r\n"));
}

#[tokio::test]
async fn untrusted_reasons_retain_the_canonical_status_line_and_complete_body() {
    for reason in [
        "invalid\r\nX-Injected: true".to_string(),
        "invalid\tvalue".to_string(),
        "invalid\0value".to_string(),
        "invalid café".to_string(),
        "x".repeat(257),
    ] {
        let response = response(RegistryError::BadRequest { reason: reason.clone() });
        assert!(
            response
                .extensions()
                .get::<ReasonPhrase>()
                .is_none(),
        );
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(String::from_utf8(bytes.to_vec()).unwrap(), format!("Bad request: {reason}"));
    }
}

#[tokio::test]
async fn internal_errors_do_not_expose_private_details_in_the_reason_phrase() {
    let response =
        response(RegistryError::Internal { reason: "private database detail".to_string() });
    assert_eq!(
        response
            .extensions()
            .get::<ReasonPhrase>()
            .unwrap()
            .as_bytes(),
        b"Internal Server Error",
    );
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(bytes.as_ref(), b"Internal Server Error");
}
