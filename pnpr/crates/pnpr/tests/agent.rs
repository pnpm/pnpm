//! Integration tests for the pnpm-agent fast-path endpoints.
//!
//! `/v1/install` resolves against an upstream registry and is covered
//! by the broader install suite; these tests exercise the network-free
//! `/v1/files` binary framing end to end through the axum router.

use std::io::Read as _;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use flate2::read::GzDecoder;
use pacquet_store_dir::StoreDir;
use pnpr::{Config, router};
use serde_json::json;
use tempfile::TempDir;
use tower::ServiceExt;

fn config_for(storage: std::path::PathBuf) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    Config::proxy(listen, storage)
}

/// Decode the `/v1/files` payload into `(digest_bytes, mode, content)`
/// frames, validating the JSON header and the end-of-stream marker.
fn parse_files_payload(payload: &[u8]) -> Vec<([u8; 64], u8, Vec<u8>)> {
    let json_len = u32::from_be_bytes(payload[0..4].try_into().unwrap()) as usize;
    assert_eq!(&payload[4..4 + json_len], b"{}");

    let mut offset = 4 + json_len;
    let mut frames = Vec::new();
    loop {
        let mut digest = [0u8; 64];
        digest.copy_from_slice(&payload[offset..offset + 64]);
        if digest == [0u8; 64] {
            break; // end-of-stream marker
        }
        let size =
            u32::from_be_bytes(payload[offset + 64..offset + 68].try_into().unwrap()) as usize;
        let mode = payload[offset + 68];
        let content_start = offset + 69;
        let content = payload[content_start..content_start + size].to_vec();
        frames.push((digest, mode, content));
        offset = content_start + size;
    }
    frames
}

#[tokio::test]
async fn files_endpoint_serves_a_cafs_file_by_digest() {
    let tmp = TempDir::new().unwrap();
    let config = config_for(tmp.path().to_path_buf());

    // Seed a file into the same content-addressable store the agent
    // runtime reads from (`<storage>/agent-store`).
    let store = StoreDir::new(tmp.path().join("agent-store"));
    let content = b"console.log('hello from the agent')\n";
    let (_path, hash) = store.write_cas_file(content, false).expect("write cas file");
    let digest = format!("{hash:x}");

    let app = router(config);
    let request_body = json!({ "digests": [{ "digest": digest, "executable": false }] });
    let response = app
        .oneshot(
            Request::post("/v1/files")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-encoding").map(|value| value.to_str().unwrap()),
        Some("gzip"),
    );

    let gzipped = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let mut decoder = GzDecoder::new(&gzipped[..]);
    let mut payload = Vec::new();
    decoder.read_to_end(&mut payload).unwrap();

    let frames = parse_files_payload(&payload);
    assert_eq!(frames.len(), 1);
    let (digest_bytes, mode, served) = &frames[0];
    assert_eq!(&served[..], content);
    assert_eq!(*mode, 0);
    assert_eq!(format!("{:x}", hash), hex(digest_bytes));
}

#[tokio::test]
async fn files_endpoint_rejects_an_invalid_digest() {
    let tmp = TempDir::new().unwrap();
    let app = router(config_for(tmp.path().to_path_buf()));

    let request_body = json!({ "digests": [{ "digest": "not-a-sha512", "executable": false }] });
    let response = app
        .oneshot(
            Request::post("/v1/files")
                .header("content-type", "application/json")
                .body(Body::from(request_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
