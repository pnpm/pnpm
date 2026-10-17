use super::{
    Body, Duration, Registries, Registry, Request, ServiceExt, StatusCode, TempDir, body_bytes,
    config_for, json, router, sha512_integrity,
};
use std::io::Write;

async fn get(app: &axum::Router, uri: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::get(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

/// A packument for `foo@1.0.0` whose tarball is `basename` on `upstream`.
fn packument(upstream: &str, basename: &str, tgz: &[u8]) -> String {
    json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": { "name": "foo", "version": "1.0.0", "dist": {
            "tarball": format!("{upstream}/foo/-/{basename}"),
            "integrity": sha512_integrity(tgz),
        } } },
    })
    .to_string()
}

/// A packument that expires at once would be fetched again by every request
/// that reads it, so a single packument fetch proves the warm request read
/// none.
#[tokio::test]
async fn a_warm_version_request_reads_no_packument() {
    let tgz = gzip(b"foo");
    let mut upstream = mockito::Server::new_async().await;
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_header("content-type", "application/json")
        .with_body(packument(&upstream.url(), "foo-1.0.0.tgz", &tgz))
        .expect(1)
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_body(&tgz)
        .expect(1)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.http.packument_ttl = Duration::ZERO;
    let app = router(config);

    for _ in 0..2 {
        let response = get(&app, "/foo@1.0.0").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response.into_body()).await, tgz);
    }
    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn a_version_whose_tarball_has_another_name_is_served() {
    let tgz = gzip(b"foo");
    let mut upstream = mockito::Server::new_async().await;
    let _packument = upstream
        .mock("GET", "/foo")
        .with_header("content-type", "application/json")
        .with_body(packument(&upstream.url(), "legacy_foo_build.tgz", &tgz))
        .create_async()
        .await;
    let _tarball = upstream
        .mock("GET", "/foo/-/legacy_foo_build.tgz")
        .with_body(&tgz)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response = get(&app, "/foo@1.0.0").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, tgz);
}

/// A tar response needs the version's `.tgz` integrity. Once read, it comes
/// from the version dist cache, so a packument that no longer parses as one
/// listing the version does not stop the next response.
#[tokio::test]
async fn a_repeated_tar_request_parses_no_packument() {
    let tgz = gzip(b"foo");
    let mut upstream = mockito::Server::new_async().await;
    let packument_body = packument(&upstream.url(), "foo-1.0.0.tgz", &tgz);
    let _packument = upstream
        .mock("GET", "/foo")
        .with_header("content-type", "application/json")
        .with_body(&packument_body)
        .create_async()
        .await;
    let _tarball = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_body(&tgz)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    let graph = vec![
        ("npmjs".to_string(), Registry::Upstream { patterns: vec![] }),
        ("zstd".to_string(), Registry::Router { sources: vec!["npmjs".to_string()] }),
    ];
    config.routing.registries = Registries::new(graph.into_iter().collect(), None);
    config.routing.tar_registries.insert("zstd".to_string());
    let app = router(config);

    let first = get(&app, "/~zstd/foo@1.0.0").await;
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(body_bytes(first.into_body()).await, b"foo");

    let cached = cached_files_containing(tmp.path(), packument_body.as_bytes());
    assert!(!cached.is_empty(), "the upstream packument is cached on disk");
    for path in cached {
        std::fs::write(path, br#"{"name":"foo","versions":{}}"#).unwrap();
    }
    let second = get(&app, "/~zstd/foo@1.0.0").await;
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(body_bytes(second.into_body()).await, b"foo");
}

fn cached_files_containing(dir: &std::path::Path, needle: &[u8]) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(cached_files_containing(&path, needle));
        } else if std::fs::read(&path).is_ok_and(|bytes| bytes == needle) {
            found.push(path);
        }
    }
    found
}
