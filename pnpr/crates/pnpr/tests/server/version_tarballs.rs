use super::{
    Body, Duration, Registries, Registry, Request, ServiceExt, StatusCode, TempDir, body_bytes,
    config_for, json, router, sha512_integrity,
};
use std::{
    io::Write,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
        let response = get(&app, "/npm/foo@1.0.0").await;
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

    let response = get(&app, "/npm/foo@1.0.0").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, tgz);
}

/// A tar registry over the default upstream.
fn tar_registry_app(upstream: &str, storage: &std::path::Path) -> axum::Router {
    let mut config = config_for(upstream, storage.to_path_buf());
    let graph = vec![
        ("npmjs".to_string(), Registry::Upstream { patterns: vec![] }),
        ("zstd".to_string(), Registry::Router { sources: vec!["npmjs".to_string()] }),
    ];
    config.routing.registries = Registries::new(graph.into_iter().collect(), None);
    config.routing.tar_registries.insert("zstd".to_string());
    router(config)
}

async fn mock_foo(upstream: &mut mockito::ServerGuard, tgz: &[u8]) -> String {
    let body = packument(&upstream.url(), "foo-1.0.0.tgz", tgz);
    upstream
        .mock("GET", "/foo")
        .with_header("content-type", "application/json")
        .with_body(&body)
        .create_async()
        .await;
    upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_body(tgz)
        .create_async()
        .await;
    body
}

/// A tar response needs the version's `.tgz` integrity. While the cached
/// packument is unchanged, it comes from the version dist cache: bytes that
/// no longer parse, written with the packument's size and time, do not stop
/// the next response. The first request caches the packument and the second
/// its dist.
#[tokio::test]
async fn a_repeated_tar_request_parses_no_packument() {
    let tgz = gzip(b"foo");
    let mut upstream = mockito::Server::new_async().await;
    let packument_body = mock_foo(&mut upstream, &tgz).await;
    let tmp = TempDir::new().unwrap();
    let app = tar_registry_app(&upstream.url(), tmp.path());

    for _ in 0..2 {
        let warm = get(&app, "/npm/~zstd/foo@1.0.0").await;
        assert_eq!(warm.status(), StatusCode::OK);
        assert_eq!(body_bytes(warm.into_body()).await, b"foo");
    }

    let cached = cached_files_containing(tmp.path(), packument_body.as_bytes());
    assert!(!cached.is_empty(), "the upstream packument is cached on disk");
    for path in cached {
        let modified = std::fs::metadata(&path)
            .unwrap()
            .modified()
            .unwrap();
        std::fs::write(&path, vec![b'x'; packument_body.len()]).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
    }
    let second = get(&app, "/npm/~zstd/foo@1.0.0").await;
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(body_bytes(second.into_body()).await, b"foo");
}

/// A cached dist describes the packument it was read from. Once that
/// packument is rewritten, the route reads the new one.
#[tokio::test]
async fn a_rewritten_packument_replaces_its_cached_dists() {
    let tgz = gzip(b"foo");
    let mut upstream = mockito::Server::new_async().await;
    let packument_body = mock_foo(&mut upstream, &tgz).await;
    let tmp = TempDir::new().unwrap();
    let app = tar_registry_app(&upstream.url(), tmp.path());

    for _ in 0..2 {
        assert_eq!(get(&app, "/npm/~zstd/foo@1.0.0").await.status(), StatusCode::OK);
    }
    for path in cached_files_containing(tmp.path(), packument_body.as_bytes()) {
        std::fs::write(path, br#"{"name":"foo","versions":{}}"#).unwrap();
    }
    assert_eq!(get(&app, "/npm/~zstd/foo@1.0.0").await.status(), StatusCode::NOT_FOUND);
}

/// The canonical filename of one version can be the `dist.tarball` another
/// version declares. A cold request still gets the requested version's own
/// tarball.
#[tokio::test]
async fn a_cold_version_request_gets_its_own_tarball() {
    let requested = gzip(b"requested");
    let other = gzip(b"other");
    let mut upstream = mockito::Server::new_async().await;
    let url = upstream.url();
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "2.0.0" },
        "versions": {
            "1.0.0": { "name": "foo", "version": "1.0.0", "dist": {
                "tarball": format!("{url}/foo/-/legacy_foo.tgz"),
                "integrity": sha512_integrity(&requested),
            } },
            "2.0.0": { "name": "foo", "version": "2.0.0", "dist": {
                "tarball": format!("{url}/foo/-/foo-1.0.0.tgz"),
                "integrity": sha512_integrity(&other),
            } },
        },
    });
    upstream
        .mock("GET", "/foo")
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .create_async()
        .await;
    upstream
        .mock("GET", "/foo/-/legacy_foo.tgz")
        .with_body(&requested)
        .create_async()
        .await;
    upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_body(&other)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&url, tmp.path().to_path_buf()));

    let response = get(&app, "/npm/foo@1.0.0").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, requested);
}

/// The version manifest of one version is a fraction of the packument, so a
/// cold request reads it instead. A tar registry keeps the dist it read.
#[tokio::test]
async fn a_cold_version_request_reads_only_its_version_manifest() {
    let tgz = gzip(b"foo");
    let mut upstream = mockito::Server::new_async().await;
    let url = upstream.url();
    let manifest = json!({ "name": "foo", "version": "1.0.0", "dist": {
        "tarball": format!("{url}/foo/-/foo-1.0.0.tgz"),
        "integrity": sha512_integrity(&tgz),
    } });
    let manifest_mock = upstream
        .mock("GET", "/foo/1.0.0")
        .with_header("content-type", "application/json")
        .with_body(manifest.to_string())
        .expect(1)
        .create_async()
        .await;
    let packument_mock = upstream
        .mock("GET", "/foo")
        .expect(0)
        .create_async()
        .await;
    upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_body(&tgz)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let app = tar_registry_app(&url, tmp.path());

    for _ in 0..2 {
        let response = get(&app, "/npm/~zstd/foo@1.0.0").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response.into_body()).await, b"foo");
    }
    manifest_mock.assert_async().await;
    packument_mock.assert_async().await;
}

/// An upstream answers a dist-tag at the version manifest URL. The route
/// serves exact versions only, so the packument decides.
#[tokio::test]
async fn a_version_manifest_of_another_version_is_not_served() {
    let tgz = gzip(b"foo");
    let mut upstream = mockito::Server::new_async().await;
    let url = upstream.url();
    let manifest = json!({ "name": "foo", "version": "1.0.0", "dist": {
        "tarball": format!("{url}/foo/-/foo-1.0.0.tgz"),
        "integrity": sha512_integrity(&tgz),
    } });
    upstream
        .mock("GET", "/foo/latest")
        .with_header("content-type", "application/json")
        .with_body(manifest.to_string())
        .create_async()
        .await;
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_header("content-type", "application/json")
        .with_body(packument(&url, "foo-1.0.0.tgz", &tgz))
        .expect(1)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&url, tmp.path().to_path_buf()));

    assert_eq!(get(&app, "/npm/foo@latest").await.status(), StatusCode::NOT_FOUND);
    packument_mock.assert_async().await;
}

/// The version names a file in the package's cache directory. One that
/// climbs out of it, even through a directory that exists, is not found.
#[tokio::test]
async fn a_version_with_path_segments_is_not_found() {
    let tgz = gzip(b"foo");
    let mut upstream = mockito::Server::new_async().await;
    let packument_body = mock_foo(&mut upstream, &tgz).await;
    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));
    assert_eq!(get(&app, "/npm/foo@1.0.0").await.status(), StatusCode::OK);

    let cached = cached_files_containing(tmp.path(), packument_body.as_bytes());
    let package_dir = cached[0].parent().unwrap();
    std::fs::create_dir(package_dir.join("foo-x")).unwrap();
    std::fs::write(tmp.path().join("secret.tgz"), b"secret").unwrap();
    let depth = package_dir
        .strip_prefix(tmp.path())
        .unwrap()
        .components()
        .count()
        + 1;
    for separator in ["%2F", "%5C"] {
        let climb = format!("..{separator}").repeat(depth);
        let response = get(&app, &format!("/npm/foo@x{separator}{climb}secret")).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "separator {separator}");
    }
}

/// A manifest whose integrity does not parse leaves the version to the
/// packument.
#[tokio::test]
async fn a_version_manifest_with_a_malformed_integrity_is_not_served() {
    let tgz = gzip(b"foo");
    let mut upstream = mockito::Server::new_async().await;
    let url = upstream.url();
    let manifest = json!({ "name": "foo", "version": "1.0.0", "dist": {
        "tarball": format!("{url}/foo/-/foo-1.0.0.tgz"),
        "integrity": "not an integrity",
    } });
    upstream
        .mock("GET", "/foo/1.0.0")
        .with_header("content-type", "application/json")
        .with_body(manifest.to_string())
        .create_async()
        .await;
    mock_foo(&mut upstream, &tgz).await;
    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&url, tmp.path().to_path_buf()));

    let response = get(&app, "/npm/foo@1.0.0").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, tgz);
}

/// An upstream that drops the manifest's connection would drop the
/// packument's too, so the stale cached packument answers at once.
#[tokio::test]
async fn an_unreachable_manifest_serves_the_stale_packument() {
    let tgz = gzip(b"foo");
    let (url, packument_fetches) = spawn_upstream_without_manifests(tgz.clone()).await;
    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&url, tmp.path().to_path_buf());
    config.http.packument_ttl = Duration::ZERO;
    let graph = vec![
        ("npmjs".to_string(), Registry::Upstream { patterns: vec![] }),
        ("zstd".to_string(), Registry::Router { sources: vec!["npmjs".to_string()] }),
    ];
    config.routing.registries = Registries::new(graph.into_iter().collect(), None);
    config.routing.tar_registries.insert("zstd".to_string());
    let app = router(config);

    for _ in 0..2 {
        let response = get(&app, "/npm/~zstd/foo@1.0.0").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response.into_body()).await, b"foo");
    }
    assert_eq!(packument_fetches.load(Ordering::SeqCst), 1);
}

/// An upstream serving `foo`'s packument and tarball. It counts packument
/// requests and closes every other connection unanswered.
async fn spawn_upstream_without_manifests(tgz: Vec<u8>) -> (String, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let packument = packument(&url, "foo-1.0.0.tgz", &tgz).into_bytes();
    let fetches = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&fetches);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let (packument, tgz, counter) = (packument.clone(), tgz.clone(), Arc::clone(&counter));
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let read = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..read]).into_owned();
                let body = if request.starts_with("GET /foo HTTP/") {
                    counter.fetch_add(1, Ordering::SeqCst);
                    packument
                } else if request.starts_with("GET /foo/-/foo-1.0.0.tgz HTTP/") {
                    tgz
                } else {
                    return;
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len(),
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&body).await;
            });
        }
    });
    (url, fetches)
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
