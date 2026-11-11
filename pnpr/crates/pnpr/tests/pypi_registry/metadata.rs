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
        ("data-dist-info-metadata", json!(true), StatusCode::OK),
        ("core-metadata", json!({"sha512": sha512_hex(bytes)}), StatusCode::OK),
        ("core-metadata", json!({"sha512": sha512_hex(b"different")}), StatusCode::BAD_GATEWAY),
        ("core-metadata", json!({}), StatusCode::NOT_FOUND),
        ("core-metadata", json!({"unsupported": "digest"}), StatusCode::NOT_FOUND),
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
            let body = body_bytes(response.into_body()).await;
            assert!(status != StatusCode::OK || body == bytes, "{field}: {value}, body: {body:?}");
        }
        let response = app
            .clone()
            .oneshot(get("/pypi/simple/sample-pkg/", Some(JSON)))
            .await
            .unwrap();
        let rendered: Value =
            serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        assert!(rendered["files"][0].get("dist-info-metadata").is_none());
        assert!(rendered["files"][0].get("data-dist-info-metadata").is_none());
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

fn sha512_hex(bytes: &[u8]) -> String {
    ssri::IntegrityOpts::new()
        .algorithm(ssri::Algorithm::Sha512)
        .chain(bytes)
        .result()
        .to_hex()
        .1
}

#[tokio::test]
async fn hosted_wheel_metadata_is_persisted_advertised_and_served_verbatim() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = super::issue_token(&auth, "publisher").await;
    let config = pypi_config(tmp.path().to_path_buf(), "http://upstream.invalid/");
    let app = router_with_auth(config.clone(), auth.clone());
    let metadata = "Metadata-Version: 2.1\r\nName: demo-pkg\r\nVersion: 1.0.0\r\nRequires-Dist: sample-dep>=2\r\n\r\nDescription\r\n";
    let wheel = super::common::python_wheel_with_metadata("demo-pkg", "1.0.0", metadata);
    let filename = "demo_pkg-1.0.0-py3-none-any.whl";
    let response = app
        .oneshot(super::upload_request(
            Some(&token),
            super::wheel_upload("demo-pkg", "1.0.0", filename, &wheel),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let app = router_with_auth(config, auth.clone());
    let digest = sha256_hex(metadata.as_bytes());
    for base in ["/pypi", "/pypi/~internal"] {
        let response = app
            .clone()
            .oneshot(get(&format!("{base}/simple/demo-pkg/"), Some(JSON)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let page: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        assert_eq!(page["files"][0]["core-metadata"], json!({"sha256": digest}));
        assert!(page["files"][0].get("hosted-metadata").is_none());
        let response = app
            .clone()
            .oneshot(get(&format!("{base}/simple/demo-pkg/"), None))
            .await
            .unwrap();
        let html = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
        assert!(html.contains(&format!(r#"data-core-metadata="sha256={digest}""#)), "{html}");
        let response = app
            .clone()
            .oneshot(get(&format!("{base}/files/demo-pkg/{filename}.metadata"), None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "text/plain; charset=utf-8");
        assert_eq!(body_bytes(response.into_body()).await, metadata.as_bytes());
    }
    let response = app
        .oneshot(get("/pypi/files/demo-pkg/missing.whl.metadata", None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let private_config = super::common::mixed_router_config(
        tmp.path().to_path_buf(),
        pnpr::Ecosystem::Pypi,
        super::common::HostedSource {
            name: "internal",
            namespace: "python",
            access: "$authenticated",
            packages: &["demo-pkg"],
        },
        ("pypiorg", "http://upstream.invalid/"),
    );
    let app = router_with_auth(private_config, auth);
    let path = format!("/pypi/files/demo-pkg/{filename}.metadata");
    let response = app
        .clone()
        .oneshot(get(&path, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let mut request = get(&path, None);
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {token}").parse().unwrap());
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, metadata.as_bytes());
}
