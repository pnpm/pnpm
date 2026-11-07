use super::{JSON, body_bytes, get, pypi_config, sha256_hex};
use axum::http::StatusCode;
use pnpr::{AuthState, router_with_auth};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

#[tokio::test]
async fn metadata_availability_legacy_fallback_and_integrity_are_enforced() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"Metadata-Version: 2.1\nName: demo-pkg\nVersion: 1.0\n";
    let filename = "sample_pkg-1.0-py3-none-any.whl";
    for (field, value, status) in [
        ("core-metadata", json!(true), StatusCode::OK),
        ("dist-info-metadata", json!({"sha256": sha256_hex(bytes)}), StatusCode::OK),
        ("core-metadata", json!(false), StatusCode::NOT_FOUND),
        ("core-metadata", Value::Null, StatusCode::NOT_FOUND),
        ("core-metadata", json!({"sha256": sha256_hex(b"different")}), StatusCode::BAD_GATEWAY),
    ] {
        let page = upstream
            .mock("GET", "/simple/sample-pkg/")
            .with_body(
                json!({
                    "name": "sample-pkg",
                    "files": [{
                        "filename": filename,
                        "url": format!("../../files/{filename}?token=sample#sha256=unused"),
                        "hashes": {"sha256": sha256_hex(b"wheel")},
                        field: value,
                    }],
                })
                .to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        let sidecar = upstream
            .mock("GET", format!("/files/{filename}.metadata?token=sample").as_str())
            .with_body(bytes)
            .expect(if status == StatusCode::NOT_FOUND {
                0
            } else if status == StatusCode::BAD_GATEWAY {
                2
            } else {
                1
            })
            .create_async()
            .await;
        let tmp = TempDir::new().unwrap();
        let app = router_with_auth(
            pypi_config(tmp.path().to_path_buf(), &format!("{}/simple/", upstream.url())),
            AuthState::in_memory(),
        );
        for _ in 0..2 {
            let response = app
                .clone()
                .oneshot(get(&format!("/pypi/files/sample-pkg/{filename}.metadata"), None))
                .await
                .unwrap();
            assert_eq!(response.status(), status, "{field}: {value}");
            assert!(status != StatusCode::OK || body_bytes(response.into_body()).await == bytes);
        }
        let response = app
            .clone()
            .oneshot(get("/pypi/simple/sample-pkg/", Some(JSON)))
            .await
            .unwrap();
        let rendered: Value =
            serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        assert_eq!(rendered["files"][0][field], value);
        page.assert_async().await;
        sidecar.assert_async().await;
        page.remove_async().await;
        sidecar.remove_async().await;
    }
}

#[tokio::test]
async fn metadata_downloads_obey_cache_settings_and_size_limits() {
    let mut upstream = mockito::Server::new_async().await;
    let filename = "sample_pkg-1.0-py3-none-any.whl";
    upstream
        .mock("GET", "/sample-pkg/")
        .with_body(
            json!({"name": "sample-pkg", "files": [{
                "filename": filename, "url": format!("/files/{filename}"),
                "core-metadata": true,
            }]})
            .to_string(),
        )
        .create_async()
        .await;
    let bytes = b"metadata";
    let sidecar = upstream
        .mock("GET", format!("/files/{filename}.metadata").as_str())
        .with_body(bytes)
        .expect(2)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let mut config = pypi_config(tmp.path().to_path_buf(), &upstream.url());
    config.routing.upstreams.get_mut("pypiorg").unwrap().cache = false;
    let app = router_with_auth(config, AuthState::in_memory());
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(get(&format!("/pypi/files/sample-pkg/{filename}.metadata"), None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response.into_body()).await, bytes);
    }
    sidecar.assert_async().await;
    sidecar.remove_async().await;
    let oversized = upstream
        .mock("GET", format!("/files/{filename}.metadata").as_str())
        .with_body(vec![b'x'; pnpr_pypi::archive::MAX_METADATA_BYTES + 1])
        .expect(2)
        .create_async()
        .await;
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(get(&format!("/pypi/files/sample-pkg/{filename}.metadata"), None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }
    oversized.assert_async().await;
}
