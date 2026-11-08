use super::{report, track};
use axum::{Router, body::Body, routing::get};
use bytes::Bytes;
use futures_util::stream;
use std::{convert::Infallible, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const FULL_BODY: &str = "a complete body";

async fn serve_tracked() -> String {
    let stalled = || async {
        let first = stream::iter([Ok::<_, Infallible>(Bytes::from_static(b"12345"))]);
        Body::from_stream(futures_util::StreamExt::chain(first, stream::pending()))
    };
    let app = Router::new()
        .route("/full", get(|| async { FULL_BODY }))
        .route("/stalled", get(stalled))
        .route("/unanswered", get(std::future::pending::<&'static str>));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let app = app.layer(axum::middleware::from_fn_with_state(address.clone(), track));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    address
}

/// Sends a GET for `path` and returns what arrives within `wait`.
async fn get_raw(address: &str, path: &str, wait: Duration) -> String {
    let mut socket = TcpStream::connect(address).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut received = Vec::new();
    let _ = tokio::time::timeout(wait, socket.read_to_end(&mut received)).await;
    String::from_utf8_lossy(&received).into_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_report_lists_unfinished_requests_and_keeps_the_framing() {
    let address = serve_tracked().await;
    let full = get_raw(&address, "/full", Duration::from_secs(5)).await;
    eprintln!("FULL RESPONSE:\n{full}");
    let stalled_client = {
        let address = address.clone();
        tokio::spawn(async move { get_raw(&address, "/stalled", Duration::from_secs(30)).await })
    };
    let unanswered_client = {
        let address = address.clone();
        tokio::spawn(async move { get_raw(&address, "/unanswered", Duration::from_secs(30)).await })
    };
    tokio::time::sleep(Duration::from_millis(500)).await;

    let report = report();
    eprintln!("REPORT:\n{report}");
    stalled_client.abort();
    unanswered_client.abort();

    assert!(full.contains(&format!("content-length: {}", FULL_BODY.len())));
    assert!(!full.contains("transfer-encoding"));
    assert!(report.contains("mock registry requests: 2 unfinished, 1 finished"));
    let line = |path: &str| {
        let prefix = format!("GET {address}{path}: started ");
        report
            .lines()
            .find(|line| line.trim_start().starts_with(&prefix))
            .unwrap_or_else(|| panic!("the report lists {path}"))
            .to_string()
    };
    assert!(line("/stalled").ends_with("body still streaming, 5 bytes sent"));
    assert!(line("/unanswered").ends_with("no response yet"));
    let full_line = line("/full");
    assert!(full_line.contains("body Done after "));
    assert!(full_line.ends_with(&format!(", {} bytes", FULL_BODY.len())));
}
