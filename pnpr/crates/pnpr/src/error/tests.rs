use std::time::Duration;

use axum::http::StatusCode;
use tokio::net::TcpListener;

use super::RegistryError;

/// `reqwest::Error` has no public constructor, so the only way to
/// get a real `is_timeout()` error in a test is to actually time
/// out. Spin up a TCP listener that accepts and holds the socket
/// open, fire a reqwest request with a sub-second timeout against
/// it, and check the error round-trips through `status_code()`.
#[tokio::test]
async fn timeout_error_maps_to_gateway_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    // Keep accepted sockets alive for the duration of the test so
    // the client really hangs on read instead of seeing FIN.
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });

    let client = reqwest::Client::builder().timeout(Duration::from_millis(100)).build().unwrap();
    let url = format!("http://{addr}/");
    let err = client.get(&url).send().await.unwrap_err();
    assert!(err.is_timeout(), "expected timeout error, got {err:?}");

    let registry_err = RegistryError::Upstream { url, source: err };
    assert_eq!(registry_err.status_code(), StatusCode::GATEWAY_TIMEOUT);
}

#[test]
fn object_store_error_maps_to_bad_gateway() {
    let err = RegistryError::ObjectStore(object_store::Error::Generic {
        store: "test",
        source: "boom".into(),
    });
    assert_eq!(err.status_code(), StatusCode::BAD_GATEWAY);
}
