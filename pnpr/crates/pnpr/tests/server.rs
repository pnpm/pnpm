use axum::{
    body::{Body, Bytes, to_bytes},
    http::{HeaderValue, Request, StatusCode, header},
};
use flate2::read::GzDecoder;
use futures_util::stream;
use pnpr::{AuthState, Config, router, router_with_auth};
use serde_json::{Value, json};
use ssri::{Algorithm, IntegrityOpts};
use std::{
    convert::Infallible,
    fs,
    io::Read as _,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tower::ServiceExt;

fn config_for(upstream: &str, storage: PathBuf) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::proxy(listen, storage);
    config.uplinks.get_mut("npmjs").expect("default `npmjs` uplink").url = upstream.to_string();
    config.public_url = "http://example.test".to_string();
    config.packument_ttl = Duration::from_mins(1);
    config
}

async fn body_bytes(body: Body) -> Vec<u8> {
    to_bytes(body, usize::MAX).await.expect("read body").to_vec()
}

fn git_resolve_request(repo_url: &str, authorization: Option<&str>) -> Request<Body> {
    let body = json!({
        "dependencies": {
            "git-dependency": format!("git+{repo_url}#main"),
        },
        "registry": "http://127.0.0.1:1/",
        "trustLockfile": true,
        "preferFrozenLockfile": false,
    });
    let mut request =
        Request::post("/-/pnpr/v0/resolve").header("content-type", "application/json");
    if let Some(authorization) = authorization {
        request = request.header("authorization", authorization);
    }
    request.body(Body::from(serde_json::to_vec(&body).unwrap())).unwrap()
}

fn verify_lockfile_request(registry_url: &str, authorization: Option<&str>) -> Request<Body> {
    let body = json!({
        "registry": registry_url,
        "lockfile": {
            "lockfileVersion": "9.0",
            "settings": {
                "autoInstallPeers": true,
                "excludeLinksFromLockfile": false,
            },
            "importers": {
                ".": {
                    "dependencies": {
                        "probe-pkg": {
                            "specifier": "1.0.0",
                            "version": "1.0.0",
                        },
                    },
                },
            },
            "packages": {
                "probe-pkg@1.0.0": {
                    "resolution": {
                        "integrity": "sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==",
                    },
                },
            },
            "snapshots": {
                "probe-pkg@1.0.0": {},
            },
        },
        "minimumReleaseAge": 1,
        "minimumReleaseAgeIgnoreMissingTime": false,
    });
    let mut request =
        Request::post("/-/pnpr/v0/verify-lockfile").header("content-type", "application/json");
    if let Some(authorization) = authorization {
        request = request.header("authorization", authorization);
    }
    request.body(Body::from(serde_json::to_vec(&body).unwrap())).unwrap()
}

async fn drain_resolve_response(response: axum::response::Response) -> (StatusCode, Vec<u8>) {
    let status = response.status();
    let body = tokio::time::timeout(Duration::from_secs(10), body_bytes(response.into_body()))
        .await
        .expect("resolver response should finish within 10 seconds");
    (status, body)
}

async fn spawn_git_probe() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let request_count = Arc::new(AtomicUsize::new(0));
    let probe_count = Arc::clone(&request_count);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            probe_count.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let _ = socket
                    .write_all(
                        b"HTTP/1.1 500 Internal Server Error\r\n\
                          Content-Length: 0\r\n\
                          Connection: close\r\n\
                          \r\n",
                    )
                    .await;
            });
        }
    });
    (format!("http://{addr}/repo.git"), request_count)
}

#[tokio::test]
async fn anonymous_resolve_cannot_trigger_git_egress() {
    let (repo_url, request_count) = spawn_git_probe().await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for("http://127.0.0.1:1", tmp.path().to_path_buf()));
    let response = app.oneshot(git_resolve_request(&repo_url, None)).await.unwrap();
    let (status, body) = drain_resolve_response(response).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(String::from_utf8_lossy(&body).contains("Authentication required"));
    assert_eq!(request_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn default_registration_cannot_mint_a_resolver_credential() {
    let (repo_url, request_count) = spawn_git_probe().await;
    let tmp = TempDir::new().unwrap();
    let app = router(config_for("http://127.0.0.1:1", tmp.path().to_path_buf()));
    let registration = json!({
        "_id": "org.couchdb.user:outsider",
        "name": "outsider",
        "password": "secret",
        "email": "outsider@example.test",
        "type": "user",
        "roles": [],
    });
    let response = app
        .clone()
        .oneshot(
            Request::put("/-/user/org.couchdb.user:outsider")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&registration).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(!String::from_utf8_lossy(&body_bytes(response.into_body()).await).contains("token"));

    let response = app.oneshot(git_resolve_request(&repo_url, None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(request_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn anonymous_resolve_is_rejected_before_the_body_is_collected() {
    let tmp = TempDir::new().unwrap();
    let app = router(config_for("http://127.0.0.1:1", tmp.path().to_path_buf()));
    let body = Body::from_stream(stream::pending::<Result<Bytes, Infallible>>());
    let request = Request::post("/-/pnpr/v0/resolve")
        .header("content-type", "application/json")
        .header("content-length", "1000000")
        .body(body)
        .unwrap();

    let response = tokio::time::timeout(Duration::from_millis(250), app.oneshot(request))
        .await
        .expect("authentication must finish without waiting for the request body")
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn resolve_rejects_duplicate_authorization_headers() {
    let (repo_url, request_count) = spawn_git_probe().await;
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config_for("http://127.0.0.1:1", tmp.path().to_path_buf()), auth);
    let mut request = git_resolve_request(&repo_url, Some(&format!("Bearer {token}")));
    request
        .headers_mut()
        .append(header::AUTHORIZATION, HeaderValue::from_static("Bearer invalid-second-value"));

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let mut reversed = git_resolve_request(&repo_url, None);
    reversed
        .headers_mut()
        .append(header::AUTHORIZATION, HeaderValue::from_static("Bearer invalid-first-value"));
    reversed
        .headers_mut()
        .append(header::AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {token}")).unwrap());
    let response = app.clone().oneshot(reversed).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let mut non_text = git_resolve_request(&repo_url, None);
    non_text.headers_mut().insert(header::AUTHORIZATION, HeaderValue::from_bytes(&[0xff]).unwrap());
    let response = app.oneshot(non_text).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(request_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn anonymous_verify_lockfile_cannot_trigger_registry_egress() {
    let (registry_url, request_count) = spawn_git_probe().await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for("http://127.0.0.1:1", tmp.path().to_path_buf()));
    let response = app.oneshot(verify_lockfile_request(&registry_url, None)).await.unwrap();
    let (status, body) = drain_resolve_response(response).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(String::from_utf8_lossy(&body).contains("Authentication required"));
    assert_eq!(request_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn anonymous_verify_lockfile_is_rejected_before_the_body_is_collected() {
    let tmp = TempDir::new().unwrap();
    let app = router(config_for("http://127.0.0.1:1", tmp.path().to_path_buf()));
    let body = Body::from_stream(stream::pending::<Result<Bytes, Infallible>>());
    let request = Request::post("/-/pnpr/v0/verify-lockfile")
        .header("content-type", "application/json")
        .header("content-length", "1000000")
        .body(body)
        .unwrap();

    let response = tokio::time::timeout(Duration::from_millis(250), app.oneshot(request))
        .await
        .expect("authentication must finish without waiting for the request body")
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn authenticated_resolve_preserves_git_dependencies() {
    let (repo_url, request_count) = spawn_git_probe().await;

    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config_for("http://127.0.0.1:1", tmp.path().to_path_buf()), auth);
    let response = app
        .oneshot(git_resolve_request(&repo_url, Some(&format!("Bearer {token}"))))
        .await
        .unwrap();
    let (status, body) = drain_resolve_response(response).await;

    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&body).contains(r#""type":"error""#));
    assert!(request_count.load(Ordering::SeqCst) >= 1);
}

async fn body_json(body: Body) -> Value {
    serde_json::from_slice(&body_bytes(body).await).expect("body parses as JSON")
}

fn sha512_integrity(bytes: &[u8]) -> String {
    let mut opts = IntegrityOpts::new().algorithm(Algorithm::Sha512);
    opts.input(bytes);
    opts.result().to_string()
}

fn osv_database(package: &str, versions: &[&str]) -> TempDir {
    let dir = TempDir::new().unwrap();
    let versions: Vec<Value> = versions.iter().map(|version| json!(version)).collect();
    let advisory = json!({
        "id": "GHSA-registry",
        "affected": [{
            "package": { "ecosystem": "npm", "name": package },
            "versions": versions,
        }],
    });
    fs::write(dir.path().join("GHSA-registry.json"), advisory.to_string()).unwrap();
    dir
}

fn enable_osv(config: &mut Config, path: &Path) {
    config.osv.enabled = true;
    config.osv.path = Some(path.to_path_buf());
}

async fn mock_packument_for_tarball(
    upstream: &mut mockito::ServerGuard,
    package: &str,
    version: &str,
    expected_bytes: &[u8],
) -> mockito::Mock {
    let basename = package.rsplit('/').next().expect("package has a basename");
    let mut versions = serde_json::Map::new();
    versions.insert(
        version.to_string(),
        json!({
            "name": package,
            "version": version,
            "dist": {
                "tarball": format!("{}/{package}/-/{basename}-{version}.tgz", upstream.url()),
                "integrity": sha512_integrity(expected_bytes),
            },
        }),
    );
    let packument = json!({
        "name": package,
        "dist-tags": { "latest": version },
        "versions": versions,
    });
    let path = format!("/{package}");
    upstream
        .mock("GET", path.as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await
}

#[tokio::test]
async fn packument_is_proxied_cached_and_rewritten() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({
        "name": "foo",
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()),
                    "shasum": "deadbeef"
                }
            }
        }
    });
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.public_url = "http://example.test".to_string();
    let app = router(config);

    let response =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(
        body["versions"]["1.0.0"]["dist"]["tarball"],
        "http://example.test/foo/-/foo-1.0.0.tgz",
    );
    assert_eq!(body["versions"]["1.0.0"]["dist"]["shasum"], "deadbeef");

    let cached =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(cached.status(), StatusCode::OK);

    packument_mock.assert_async().await;
}

#[tokio::test]
async fn osv_filters_vulnerable_versions_from_proxy_and_cache() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.1.0", "stable": "1.0.0" },
        "time": {
            "modified": "2026-06-21T12:00:00.000Z",
            "1.0.0": "2026-06-20T12:00:00.000Z",
            "1.1.0": "2026-06-21T12:00:00.000Z",
        },
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": { "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()) },
            },
            "1.1.0": {
                "name": "foo",
                "version": "1.1.0",
                "dist": { "tarball": format!("{}/foo/-/foo-1.1.0.tgz", upstream.url()) },
            },
        },
    });
    let mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let osv = osv_database("foo", &["1.1.0"]);
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.resolver.enabled = false;
    enable_osv(&mut config, osv.path());
    let app = router(config);

    let first =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let body = body_json(first.into_body()).await;
    assert!(body["versions"].get("1.1.0").is_none());
    assert!(body["time"].get("1.1.0").is_none());
    assert_eq!(body["versions"]["1.0.0"]["version"], "1.0.0");
    assert!(body["dist-tags"].get("latest").is_none());
    assert_eq!(body["dist-tags"]["stable"], "1.0.0");

    let cached =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(cached.status(), StatusCode::OK);
    let cached_body = body_json(cached.into_body()).await;
    assert!(cached_body["versions"].get("1.1.0").is_none());
    assert!(cached_body["time"].get("1.1.0").is_none());
    assert!(cached_body["dist-tags"].get("latest").is_none());
    assert_eq!(cached_body["dist-tags"]["stable"], "1.0.0");

    let vulnerable_manifest =
        app.clone().oneshot(Request::get("/foo/1.1.0").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(vulnerable_manifest.status(), StatusCode::NOT_FOUND);

    let safe_manifest =
        app.clone().oneshot(Request::get("/foo/1.0.0").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(safe_manifest.status(), StatusCode::OK);
    let safe_body = body_json(safe_manifest.into_body()).await;
    assert_eq!(safe_body["version"], "1.0.0");

    let dist_tags = app
        .oneshot(Request::get("/-/package/foo/dist-tags").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(dist_tags.status(), StatusCode::OK);
    let tags = body_json(dist_tags.into_body()).await;
    assert!(tags.get("latest").is_none());
    assert_eq!(tags["stable"], "1.0.0");

    mock.assert_async().await;
}

#[tokio::test]
async fn osv_filters_packument_identity_mismatches() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({
        "name": "foo",
        "dist-tags": {
            "latest": "1.1.0",
            "alias": "safe-key",
            "hidden": "1.2.0",
            "stable": "1.0.0",
        },
        "time": {
            "modified": "2026-06-21T12:00:00.000Z",
            "1.0.0": "2026-06-20T12:00:00.000Z",
            "1.1.0": "2026-06-21T12:00:00.000Z",
            "safe-key": "2026-06-21T12:00:00.000Z",
            "1.2.0": "2026-06-21T12:00:00.000Z",
        },
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": { "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()) },
            },
            "1.1.0": {
                "name": "foo",
                "version": "9.9.9",
                "dist": { "tarball": format!("{}/foo/-/foo-1.1.0.tgz", upstream.url()) },
            },
            "safe-key": {
                "name": "foo",
                "version": "1.2.0",
                "dist": { "tarball": format!("{}/foo/-/foo-1.2.0.tgz", upstream.url()) },
            },
        },
    });
    let mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let osv = osv_database("foo", &["1.1.0", "1.2.0"]);
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.resolver.enabled = false;
    enable_osv(&mut config, osv.path());
    let app = router(config);

    let response =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    let versions = body["versions"].as_object().unwrap();
    assert_eq!(versions.len(), 1);
    assert!(versions.contains_key("1.0.0"));
    let tags = body["dist-tags"].as_object().unwrap();
    assert_eq!(tags.len(), 1);
    assert!(tags.contains_key("stable"));
    let time = body["time"].as_object().unwrap();
    assert_eq!(time.len(), 2);
    assert!(time.contains_key("modified"));
    assert!(time.contains_key("1.0.0"));

    let vulnerable_key_manifest =
        app.clone().oneshot(Request::get("/foo/1.1.0").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(vulnerable_key_manifest.status(), StatusCode::NOT_FOUND);
    let vulnerable_manifest_version = app
        .clone()
        .oneshot(Request::get("/foo/safe-key").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(vulnerable_manifest_version.status(), StatusCode::NOT_FOUND);

    let dist_tags = app
        .oneshot(Request::get("/-/package/foo/dist-tags").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(dist_tags.status(), StatusCode::OK);
    let tags = body_json(dist_tags.into_body()).await;
    assert_eq!(tags.as_object().unwrap().len(), 1);
    assert_eq!(tags["stable"], "1.0.0");

    mock.assert_async().await;
}

#[tokio::test]
async fn uplink_auth_and_custom_headers_are_forwarded_upstream() {
    let mut upstream = mockito::Server::new_async().await;
    // The mock only matches when both headers are present, so a passing
    // request proves the resolved per-uplink headers reached upstream.
    let mock = upstream
        .mock("GET", "/foo")
        .match_header("authorization", "Bearer secret-token")
        .match_header("x-org", "acme")
        .with_status(200)
        .with_body(json!({ "name": "foo", "versions": {} }).to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    let uplink = config.uplinks.get_mut("npmjs").expect("default `npmjs` uplink");
    uplink.headers.insert("authorization", "Bearer secret-token".parse().unwrap());
    uplink.headers.insert("x-org", "acme".parse().unwrap());
    let app = router(config);

    let response = app.oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    mock.assert_async().await;
}

#[tokio::test]
async fn scoped_packument_is_served() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({ "name": "@types/node", "versions": {} });
    let mock = upstream
        .mock("GET", "/@types/node")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response =
        app.oneshot(Request::get("/@types/node").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    mock.assert_async().await;
}

#[tokio::test]
async fn tarball_is_proxied_and_cached() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"fake-tarball-bytes";
    let packument_mock = mock_packument_for_tarball(&mut upstream, "foo", "1.0.0", bytes).await;
    let mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_header("content-type", "application/octet-stream")
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(config_for(&upstream.url(), storage.clone()));

    let first = app
        .clone()
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(body_bytes(first.into_body()).await, bytes);

    let second = router(config_for("http://127.0.0.1:1", storage))
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(body_bytes(second.into_body()).await, bytes);

    packument_mock.assert_async().await;
    mock.assert_async().await;
}

#[tokio::test]
async fn osv_refuses_vulnerable_tarball_before_upstream_fetch() {
    let mut upstream = mockito::Server::new_async().await;
    let mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body("vulnerable tarball")
        .expect(0)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let osv = osv_database("foo", &["1.0.0"]);
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.resolver.enabled = false;
    enable_osv(&mut config, osv.path());
    let app = router(config);

    let response = app
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(body.contains("GHSA-registry"), "{body}");

    mock.assert_async().await;
}

#[tokio::test]
async fn osv_tarball_screening_preserves_access_gate() {
    let mut upstream = mockito::Server::new_async().await;
    let mock = upstream
        .mock("GET", "/@pnpm.e2e/needs-auth/-/needs-auth-1.0.0.tgz")
        .with_status(200)
        .with_body("private vulnerable tarball")
        .expect(0)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let osv = osv_database("@pnpm.e2e/needs-auth", &["1.0.0"]);
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.resolver.enabled = false;
    enable_osv(&mut config, osv.path());
    let app = router(config);

    let response = app
        .oneshot(
            Request::get("/@pnpm.e2e/needs-auth/-/needs-auth-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    mock.assert_async().await;
}

#[tokio::test]
async fn osv_refuses_vulnerable_tarball_from_cache() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"cached vulnerable tarball";
    let packument_mock = mock_packument_for_tarball(&mut upstream, "foo", "1.0.0", bytes).await;
    let mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();
    let warming_app = router(config_for(&upstream.url(), cache_dir.clone()));

    let warmed = warming_app
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(warmed.status(), StatusCode::OK);
    assert_eq!(body_bytes(warmed.into_body()).await, bytes);

    let osv = osv_database("foo", &["1.0.0"]);
    let mut config = config_for(&upstream.url(), cache_dir);
    config.resolver.enabled = false;
    enable_osv(&mut config, osv.path());
    let screened_app = router(config);

    let response = screened_app
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    packument_mock.assert_async().await;
    mock.assert_async().await;
}

#[tokio::test]
async fn tarball_route_preserves_basename_and_binds_to_declaring_version() {
    // A version's tarball is served, fetched, and cached under the basename
    // its own `dist.tarball` declares (preserved verbatim, so a
    // non-canonical upstream name survives into the client's lockfile). The
    // bytes are verified against that declaring version's integrity, so the
    // preserved name still can't smuggle in bytes of another provenance.
    // Here the packument deliberately swaps the two versions' tarball names
    // to prove the binding follows the declaring version, not the filename.
    let mut upstream = mockito::Server::new_async().await;
    let v1_bytes = b"selected-version-one";
    let v2_bytes = b"other-version-two";
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("{}/foo/-/foo-2.0.0.tgz", upstream.url()),
                    "integrity": sha512_integrity(v1_bytes),
                },
            },
            "2.0.0": {
                "name": "foo",
                "version": "2.0.0",
                "dist": {
                    "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()),
                    "integrity": sha512_integrity(v2_bytes),
                },
            },
        },
    });
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    // `latest` is 1.0.0, whose declared tarball basename is `foo-2.0.0.tgz`,
    // so that is the upstream path pnpr fetches — and it must yield bytes
    // matching 1.0.0's integrity.
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-2.0.0.tgz")
        .with_status(200)
        .with_body(v1_bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(config_for(&upstream.url(), storage.clone()));

    let selected = app
        .clone()
        .oneshot(Request::get("/foo/latest").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(selected.status(), StatusCode::OK);
    let selected: Value = serde_json::from_slice(&body_bytes(selected.into_body()).await).unwrap();
    assert_eq!(selected["dist"]["tarball"], "http://example.test/foo/-/foo-2.0.0.tgz");

    let full =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    let full: Value = serde_json::from_slice(&body_bytes(full.into_body()).await).unwrap();
    assert_eq!(
        full["versions"]["1.0.0"]["dist"]["tarball"],
        "http://example.test/foo/-/foo-2.0.0.tgz",
    );
    assert_eq!(
        full["versions"]["2.0.0"]["dist"]["tarball"],
        "http://example.test/foo/-/foo-1.0.0.tgz",
    );

    let route =
        selected["dist"]["tarball"].as_str().unwrap().strip_prefix("http://example.test").unwrap();
    let first = app.oneshot(Request::get(route).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(body_bytes(first.into_body()).await, v1_bytes);

    let package_dir = storage.join(".pnpr-cache").join("foo");
    assert_eq!(tarball_cache_entries(&package_dir), vec!["foo-2.0.0.tgz".to_string()]);

    let offline = router(config_for("http://127.0.0.1:1", storage));
    let replay = offline.oneshot(Request::get(route).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(body_bytes(replay.into_body()).await, v1_bytes);

    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn tampered_upstream_tarball_is_rejected_and_not_cached() {
    let mut upstream = mockito::Server::new_async().await;
    let good_bytes = b"good-tarball-bytes";
    let poison_bytes = b"poisoned-cache-bytes";
    let packument = json!({
        "name": "poisoned",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "poisoned",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!(
                        "{}/poisoned/-/poisoned-1.0.0.tgz",
                        upstream.url()
                    ),
                    "integrity": sha512_integrity(good_bytes),
                },
            },
        },
    });
    let packument_mock = upstream
        .mock("GET", "/poisoned")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", "/poisoned/-/poisoned-1.0.0.tgz")
        .with_status(200)
        .with_header("content-type", "application/octet-stream")
        .with_body(poison_bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let cache_dir = storage.join(".pnpr-cache").join("poisoned");
    std::fs::create_dir_all(&cache_dir).unwrap();
    let cache_path = cache_dir.join("poisoned-1.0.0.tgz");
    std::fs::write(&cache_path, poison_bytes).unwrap();

    let app = router(config_for(&upstream.url(), storage.clone()));
    let packument_response =
        app.clone().oneshot(Request::get("/poisoned").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(packument_response.status(), StatusCode::OK);

    let tarball_response = app
        .oneshot(Request::get("/poisoned/-/poisoned-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(tarball_response.status(), StatusCode::BAD_GATEWAY);
    assert!(!cache_path.exists(), "unverified tarball must not remain cached");

    let cached_response = router(config_for("http://127.0.0.1:1", storage))
        .oneshot(Request::get("/poisoned/-/poisoned-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(cached_response.status(), StatusCode::SERVICE_UNAVAILABLE);

    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn tarball_without_integrity_is_rejected_before_fetch() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = foo_packument(&upstream.url());
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body("unverified")
        .expect(0)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let response = router(config_for(&upstream.url(), tmp.path().to_path_buf()))
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn ambiguous_tarball_basename_is_rejected_before_fetch() {
    // Two versions declaring the same dist.tarball basename make the
    // declaring version ambiguous, so the request must fail closed rather
    // than bind integrity/OSV to whichever version is encountered first.
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"shared-basename-bytes";
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()),
                    "integrity": sha512_integrity(bytes),
                },
            },
            "2.0.0": {
                "name": "foo",
                "version": "2.0.0",
                "dist": {
                    "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()),
                    "integrity": sha512_integrity(bytes),
                },
            },
        },
    });
    let packument_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(0)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let response = router(config_for(&upstream.url(), tmp.path().to_path_buf()))
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn invalid_tarball_integrities_are_controlled_failures() {
    for (case, integrity) in [
        ("malformed", "not-a-valid-sri"),
        ("whitespace", " \t\n "),
        ("zero-hash", ""),
        ("unsupported", "md5-deadbeef"),
    ] {
        let mut upstream = mockito::Server::new_async().await;
        let packument = json!({
            "name": "foo",
            "versions": {
                "1.0.0": {
                    "name": "foo",
                    "version": "1.0.0",
                    "dist": {
                        "tarball": format!("{}/foo/-/foo-1.0.0.tgz", upstream.url()),
                        "integrity": integrity,
                    },
                },
            },
        });
        let packument_mock = upstream
            .mock("GET", "/foo")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(packument.to_string())
            .expect(1)
            .create_async()
            .await;
        let tarball_mock = upstream
            .mock("GET", "/foo/-/foo-1.0.0.tgz")
            .with_status(200)
            .with_body("must-not-be-fetched")
            .expect(0)
            .create_async()
            .await;

        let tmp = TempDir::new().unwrap();
        let response = router(config_for(&upstream.url(), tmp.path().to_path_buf()))
            .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "case: {case}");
        packument_mock.assert_async().await;
        tarball_mock.assert_async().await;
    }
}

#[tokio::test]
async fn upstream_404_is_propagated() {
    let mut upstream = mockito::Server::new_async().await;
    let _mock = upstream
        .mock("GET", "/missing")
        .with_status(404)
        .with_body("Not Found")
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response =
        app.oneshot(Request::get("/missing").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tarball_verification_finalizes_cache_with_no_tmp_leftover() {
    let mut upstream = mockito::Server::new_async().await;
    // Large-ish body so the streaming path is exercised across many
    // chunks rather than fitting in a single hyper buffer.
    let bytes = vec![0xAB_u8; 512 * 1024];
    let _packument_mock = mock_packument_for_tarball(&mut upstream, "big", "1.0.0", &bytes).await;
    let _mock = upstream
        .mock("GET", "/big/-/big-1.0.0.tgz")
        .with_status(200)
        .with_body(&bytes)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();
    let app = router(config_for(&upstream.url(), cache_dir.clone()));

    let response = app
        .oneshot(Request::get("/big/-/big-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let received = body_bytes(response.into_body()).await;
    assert_eq!(received.len(), bytes.len());
    assert_eq!(received, bytes);

    // Verification and finalization complete before the response is
    // built, leaving only the canonical cache path.
    let package_dir = cache_dir.join(".pnpr-cache").join("big");
    let entries = tarball_cache_entries(&package_dir);
    assert_eq!(entries, vec!["big-1.0.0.tgz".to_string()]);

    let cached = std::fs::read(package_dir.join("big-1.0.0.tgz")).unwrap();
    assert_eq!(cached.len(), bytes.len());
    assert_eq!(cached, bytes);
}

#[tokio::test]
async fn upstream_5xx_maps_to_bad_gateway() {
    let mut upstream = mockito::Server::new_async().await;
    let _mock =
        upstream.mock("GET", "/broken").with_status(500).with_body("kaboom").create_async().await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response = app.oneshot(Request::get("/broken").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn unreachable_upstream_maps_to_service_unavailable() {
    // Bind a TCP listener and immediately drop it so the port is
    // (very likely) free; pointing the registry at a port nothing is
    // listening on exercises the `is_connect()` branch of the status
    // mapping without depending on DNS.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_port = listener.local_addr().unwrap().port();
    drop(listener);
    let dead_upstream = format!("http://127.0.0.1:{dead_port}");

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&dead_upstream, tmp.path().to_path_buf()));

    let response = app.oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn tarball_filename_for_other_package_is_rejected() {
    let upstream = mockito::Server::new_async().await;
    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response = app
        .oneshot(Request::get("/foo/-/bar-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn scoped_tarball_is_proxied() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"scoped-tarball-bytes";
    let _packument_mock =
        mock_packument_for_tarball(&mut upstream, "@types/node", "20.0.0", bytes).await;
    let mock = upstream
        .mock("GET", "/@types/node/-/node-20.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response = app
        .oneshot(Request::get("/@types/node/-/node-20.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, bytes);
    mock.assert_async().await;
}

#[tokio::test]
async fn scoped_tarball_filename_is_canonicalized_before_fetch_and_cache() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"scoped-tarball-full-name";
    let packument_mock =
        mock_packument_for_tarball(&mut upstream, "@types/node", "20.0.0", bytes).await;
    let mock = upstream
        .mock("GET", "/@types/node/-/node-20.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(config_for(&upstream.url(), storage.clone()));

    let noncanonical = app
        .clone()
        .oneshot(
            Request::get("/@types/node/-/%40types%2Fnode-20.0.0.tgz").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(noncanonical.status(), StatusCode::OK);
    assert_eq!(body_bytes(noncanonical.into_body()).await, bytes);

    let canonical = app
        .oneshot(Request::get("/@types/node/-/node-20.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(canonical.status(), StatusCode::OK);
    assert_eq!(body_bytes(canonical.into_body()).await, bytes);
    assert!(storage.join(".pnpr-cache/@types/node/node-20.0.0.tgz").exists());
    assert!(!storage.join(".pnpr-cache/@types/node/@types").exists());
    packument_mock.assert_async().await;
    mock.assert_async().await;
}

#[tokio::test]
async fn packument_is_refetched_after_ttl_expires() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({ "name": "foo", "versions": {} });
    let mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(2)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.packument_ttl = Duration::from_millis(50);
    let app = router(config);

    let r1 = app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r1.status(), StatusCode::OK);
    let _ = body_bytes(r1.into_body()).await;

    // Wait past the TTL so the cached packument is stale.
    tokio::time::sleep(Duration::from_millis(120)).await;

    let r2 = app.oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r2.status(), StatusCode::OK);

    // Mock asserts exactly 2 upstream calls were made.
    mock.assert_async().await;
}

/// A stale cached packument is revalidated with a conditional GET: the
/// upstream's `ETag` is replayed as `If-None-Match`, and a `304` lets
/// the server serve its cached copy without re-downloading the body. The
/// `304` also refreshes the entry, so a third request inside the TTL is
/// served straight from cache with no upstream call at all.
#[tokio::test]
async fn stale_packument_is_revalidated_with_conditional_get() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({ "name": "foo", "dist-tags": { "latest": "1.0.0" }, "versions": {} });
    // First request (no validator yet): full body plus an ETag to store.
    let full = upstream
        .mock("GET", "/foo")
        .match_header("if-none-match", mockito::Matcher::Missing)
        .with_status(200)
        .with_header("etag", r#""v1""#)
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    // Revalidation: the stored ETag comes back as If-None-Match; upstream
    // confirms it's unchanged with a bodyless 304.
    let revalidate = upstream
        .mock("GET", "/foo")
        .match_header("if-none-match", r#""v1""#)
        .with_status(304)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    // A generous TTL: r2's `304` refresh rewrites the cache file, and r3 must
    // see that entry as fresh. The margin has to comfortably exceed the wall
    // time between r2's refresh-write and r3's freshness check, which can spike
    // on a contended CI runner (and is subject to coarse filesystem mtime
    // granularity on Windows). The r1->r2 sleep stays longer than the TTL so r2
    // is still stale.
    config.packument_ttl = Duration::from_millis(500);
    let app = router(config);

    let r1 = app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r1.status(), StatusCode::OK);
    let _ = body_bytes(r1.into_body()).await;

    // Wait past the TTL so the cached packument is stale and gets revalidated.
    tokio::time::sleep(Duration::from_millis(700)).await;

    let r2 = app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r2.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(r2.into_body()).await).unwrap();
    assert_eq!(body["dist-tags"]["latest"], "1.0.0", "304 must serve the cached body");

    // The 304 refreshed the entry, so this request is within the TTL and
    // never reaches the upstream — both mocks asserting exactly one call.
    let r3 = app.oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r3.status(), StatusCode::OK);

    full.assert_async().await;
    revalidate.assert_async().await;
}

/// A hosted packument is authoritative: even with a proxy upstream
/// configured, a divergent upstream packument, and an expired TTL, the
/// hosted copy is served verbatim and the upstream is never contacted.
/// This is the guarantee that published versions can't be masked or
/// lost by a proxy refresh.
#[tokio::test]
async fn hosted_packument_is_never_overwritten_by_upstream() {
    let mut upstream = mockito::Server::new_async().await;
    let upstream_mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(json!({ "name": "foo", "versions": { "9.9.9": {} } }).to_string())
        .expect(0)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();

    // Seed the hosted store directly, as a publish (or static fixture)
    // would have. The hosted root is `config.storage` == tmp.
    let hosted = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": { "name": "foo", "version": "1.0.0", "dist": {
                "tarball": "http://example.test/foo/-/foo-1.0.0.tgz", "shasum": "abc"
            }},
        },
    });
    std::fs::create_dir_all(tmp.path().join("foo")).unwrap();
    std::fs::write(tmp.path().join("foo/package.json"), hosted.to_string()).unwrap();

    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    // Zero TTL: if the hosted copy were treated as a cache entry, this
    // would force an immediate upstream refetch.
    config.packument_ttl = Duration::from_millis(0);
    let app = router(config);

    let response = app.oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    let versions: Vec<&str> =
        body["versions"].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(versions, vec!["1.0.0"], "hosted packument must win over upstream's 9.9.9");

    // The upstream was never hit — the hosted copy short-circuits the proxy.
    upstream_mock.assert_async().await;
}

#[tokio::test]
async fn stale_packument_is_served_when_upstream_fails() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({ "name": "foo", "dist-tags": { "latest": "1.0.0" }, "versions": {} });
    let _mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();

    // Round 1: warm the cache against a working upstream.
    let r1 = router(config_for(&upstream.url(), cache_dir.clone()))
        .oneshot(Request::get("/foo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(r1.status(), StatusCode::OK);
    let _ = body_bytes(r1.into_body()).await;

    // Round 2: point at a dead port (so upstream errors) and set TTL
    // to zero so the cache is considered stale. The handler should
    // try upstream, fail, and fall back to the on-disk packument.
    let mut dead_config = config_for("http://127.0.0.1:1", cache_dir.clone());
    dead_config.packument_ttl = Duration::from_millis(0);
    let r2 = router(dead_config)
        .oneshot(Request::get("/foo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(r2.status(), StatusCode::OK, "stale cache should be served on upstream failure");
    let body: Value = serde_json::from_slice(&body_bytes(r2.into_body()).await).unwrap();
    assert_eq!(body["name"], "foo");
    assert_eq!(body["dist-tags"]["latest"], "1.0.0");
}

#[tokio::test]
async fn invalid_package_name_returns_bad_request() {
    let upstream = mockito::Server::new_async().await;
    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    // `.hidden` trips the dot-prefix rejection in `PackageName::parse`.
    let response =
        app.oneshot(Request::get("/.hidden").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn concurrent_tarball_fetches_settle_to_one_cache_file() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = vec![0xCD; 128 * 1024];
    let _packument_mock = mock_packument_for_tarball(&mut upstream, "foo", "1.0.0", &bytes).await;
    let mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(&bytes)
        .expect_at_least(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();
    let app = router(config_for(&upstream.url(), cache_dir.clone()));

    let packument =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(packument.status(), StatusCode::OK);

    let req1 =
        app.clone().oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap());
    let req2 =
        app.clone().oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap());
    let (r1, r2) = tokio::join!(req1, req2);
    let (r1, r2) = (r1.unwrap(), r2.unwrap());
    assert_eq!(r1.status(), StatusCode::OK);
    assert_eq!(r2.status(), StatusCode::OK);
    assert_eq!(body_bytes(r1.into_body()).await, bytes);
    assert_eq!(body_bytes(r2.into_body()).await, bytes);

    // Any overlapping verified writes atomically target the same final
    // path, so one tarball remains with no temporary siblings.
    let dir = cache_dir.join(".pnpr-cache").join("foo");
    let entries = tarball_cache_entries(&dir);
    assert_eq!(entries, vec!["foo-1.0.0.tgz".to_string()]);

    mock.assert_async().await;
}

#[tokio::test]
async fn cache_tmp_open_failure_fails_closed() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"served-without-cache";
    let _packument_mock = mock_packument_for_tarball(&mut upstream, "foo", "1.0.0", bytes).await;
    let _mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .create_async()
        .await;

    // Point `cache_dir` at a regular file so `create_dir_all` inside
    // `open_cached_tarball_tmp` fails. Without a place to verify the
    // complete body, the handler must fail rather than stream it.
    let tmp = TempDir::new().unwrap();
    let blocked = tmp.path().join("not-a-dir");
    std::fs::write(&blocked, b"already a file").unwrap();

    let app = router(config_for(&upstream.url(), blocked.clone()));

    let response = app
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

    // The cache path under the not-a-dir should not exist.
    let cache_path = blocked.join(".pnpr-cache").join("foo").join("foo-1.0.0.tgz");
    assert!(!cache_path.exists());
}

#[tokio::test]
async fn hosted_tarball_open_failure_fails_closed() {
    let mut upstream = mockito::Server::new_async().await;
    // A hosted-store fault must fail closed before any upstream lookup,
    // so neither the packument nor the tarball may be fetched.
    let packument_mock = upstream.mock("GET", "/foo").expect(0).create_async().await;
    let tarball_mock = upstream.mock("GET", "/foo/-/foo-1.0.0.tgz").expect(0).create_async().await;

    // Place a regular file where the hosted package directory belongs so
    // opening the hosted tarball fails with a real I/O error (not a plain
    // miss). The authoritative hosted store must fail closed instead of
    // falling through to the upstream proxy.
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    std::fs::create_dir_all(&storage).unwrap();
    std::fs::write(storage.join("foo"), b"not a directory").unwrap();

    let response = router(config_for(&upstream.url(), storage))
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn malformed_upstream_json_maps_to_bad_gateway() {
    let mut upstream = mockito::Server::new_async().await;
    let _mock = upstream
        .mock("GET", "/borked")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("<html>upstream CDN error page</html>")
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response = app.oneshot(Request::get("/borked").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
}

/// Spawn a TCP listener that serves a valid packument but truncates the
/// matching tarball body. Mockito cannot simulate a mid-body disconnect.
async fn spawn_truncated_upstream(expected_integrity: String) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let packument = json!({
        "name": "foo",
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("http://{addr}/foo/-/foo-1.0.0.tgz"),
                    "integrity": expected_integrity,
                },
            },
        },
    })
    .to_string();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let packument = packument.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let bytes_read = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..bytes_read]);
                if request.starts_with("GET /foo HTTP/") {
                    let response = format!(
                        "HTTP/1.1 200 OK\r\n\
                         Content-Length: {}\r\n\
                         Content-Type: application/json\r\n\
                         Connection: close\r\n\
                         \r\n\
                         {packument}",
                        packument.len(),
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    return;
                }
                if request.starts_with("GET /foo/-/foo-1.0.0.tgz HTTP/") {
                    let _ = socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\n\
                          Content-Length: 1048576\r\n\
                          Content-Type: application/octet-stream\r\n\
                          Connection: close\r\n\
                          \r\n",
                        )
                        .await;
                    let _ = socket.write_all(&[0xAA; 100]).await;
                    return;
                }
                let _ =
                    socket.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await;
            });
        }
    });
    addr
}

#[tokio::test]
async fn upstream_stream_error_clears_cache() {
    let expected_bytes = vec![0xAA; 1024 * 1024];
    let addr = spawn_truncated_upstream(sha512_integrity(&expected_bytes)).await;
    let upstream_url = format!("http://{addr}");
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();
    let app = router(config_for(&upstream_url, cache_dir.clone()));

    let response = app
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

    // The incomplete body is rejected before a response or cache entry
    // can expose its bytes.
    assert!(await_no_tgz(&cache_dir.join(".pnpr-cache").join("foo"), Duration::from_secs(1)).await);
}

#[tokio::test]
async fn verified_tarball_is_cached_before_response_is_served() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = vec![0xEE_u8; 512 * 1024];
    let _packument_mock = mock_packument_for_tarball(&mut upstream, "big", "1.0.0", &bytes).await;
    let _mock = upstream
        .mock("GET", "/big/-/big-1.0.0.tgz")
        .with_status(200)
        .with_body(&bytes)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();
    let app = router(config_for(&upstream.url(), cache_dir.clone()));

    let response = app
        .oneshot(Request::get("/big/-/big-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // The response is not constructed until the complete upstream body
    // has passed verification and cache finalization.
    drop(response);
    let cache_path = cache_dir.join(".pnpr-cache").join("big").join("big-1.0.0.tgz");
    assert_eq!(std::fs::read(cache_path).unwrap(), bytes);
}

fn is_tarball_tmp(name: &str) -> bool {
    name.split_once(".tgz.tmp.").is_some_and(|(_, suffix)| !suffix.is_empty())
}

fn tarball_cache_entries(dir: &std::path::Path) -> Vec<String> {
    let mut entries = dir
        .read_dir()
        .map(|iter| {
            iter.filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".tgz") || is_tarball_tmp(name))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    entries.sort();
    entries
}

async fn await_no_tgz(dir: &std::path::Path, budget: Duration) -> bool {
    let deadline = std::time::Instant::now() + budget;
    loop {
        if tarball_cache_entries(dir).is_empty() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn ping_endpoint_returns_json_empty_object() {
    let tmp = TempDir::new().unwrap();
    let config = config_for("http://upstream.invalid", tmp.path().to_path_buf());
    let app = router(config);

    let response = app.oneshot(Request::get("/-/ping").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = body_bytes(response.into_body()).await;
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body, json!({}));
}

fn foo_packument(upstream_url: &str) -> Value {
    json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "foo",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("{upstream_url}/foo/-/foo-1.0.0.tgz"),
                    "shasum": "deadbeef",
                },
            },
        },
    })
}

#[tokio::test]
async fn packument_is_gzipped_for_clients_that_accept_it() {
    let mut upstream = mockito::Server::new_async().await;
    let mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(foo_packument(&upstream.url()).to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response = app
        .oneshot(
            Request::get("/foo").header("accept-encoding", "gzip").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-encoding").and_then(|value| value.to_str().ok()),
        Some("gzip"),
        "a packument should be gzipped when the client accepts gzip",
    );

    // Decoding yields the same rewritten JSON a plain request would return.
    let gzipped = body_bytes(response.into_body()).await;
    let mut decoded = Vec::new();
    GzDecoder::new(&gzipped[..]).read_to_end(&mut decoded).expect("decode gzip");
    let body: Value = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(
        body["versions"]["1.0.0"]["dist"]["tarball"],
        "http://example.test/foo/-/foo-1.0.0.tgz",
    );

    mock.assert_async().await;
}

#[tokio::test]
async fn packument_is_not_gzipped_without_accept_encoding() {
    let mut upstream = mockito::Server::new_async().await;
    let mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(foo_packument(&upstream.url()).to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response = app.oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers().get("content-encoding").is_none(),
        "no Accept-Encoding means the packument is served uncompressed",
    );
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(body["name"], "foo");

    mock.assert_async().await;
}

#[tokio::test]
async fn tarball_is_not_gzipped_even_when_accepted() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"fake-tarball-bytes-long-enough-to-clear-the-compression-size-floor";
    let _packument_mock = mock_packument_for_tarball(&mut upstream, "foo", "1.0.0", bytes).await;
    let mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_header("content-type", "application/octet-stream")
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    // Tarballs are already `.tgz` (gzip); the layer must not re-compress
    // them even when the client offers `Accept-Encoding: gzip`.
    let response = app
        .oneshot(
            Request::get("/foo/-/foo-1.0.0.tgz")
                .header("accept-encoding", "gzip")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers().get("content-encoding").is_none(),
        "tarballs must not be re-gzipped",
    );
    assert_eq!(body_bytes(response.into_body()).await, bytes);

    mock.assert_async().await;
}

/// A per-uplink `maxage` shorter than the global `packument_ttl` governs
/// freshness: with a generous global TTL the second request would be a
/// cache hit, but `maxage: 0` forces a revalidation, so the upstream is
/// hit twice.
#[tokio::test]
async fn per_uplink_maxage_overrides_global_packument_ttl() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({ "name": "foo", "versions": {} });
    let mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(2)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    // Global TTL stays generous (a minute); the per-uplink maxage of zero
    // is what must take effect and make every read stale.
    config.packument_ttl = Duration::from_mins(1);
    config.uplinks.get_mut("npmjs").expect("default `npmjs` uplink").maxage =
        Some(Duration::from_millis(0));
    let app = router(config);

    let r1 = app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r1.status(), StatusCode::OK);
    let _ = body_bytes(r1.into_body()).await;

    let r2 = app.oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(r2.status(), StatusCode::OK);

    mock.assert_async().await;
}

/// A `cache: false` uplink streams tarballs through without writing them
/// to the local mirror: a second request re-fetches from the upstream
/// (so the mock is hit twice) and no `.tgz` is left in the cache dir.
#[tokio::test]
async fn cache_false_uplink_streams_tarball_without_mirroring() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"uncached-tarball-bytes";
    let packument_mock = mock_packument_for_tarball(&mut upstream, "foo", "1.0.0", bytes).await;
    let mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect(2)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();
    let mut config = config_for(&upstream.url(), cache_dir.clone());
    config.uplinks.get_mut("npmjs").expect("default `npmjs` uplink").cache = false;
    let app = router(config);

    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response.into_body()).await, bytes);
    }

    // Nothing was mirrored: the package dir either doesn't exist or holds
    // no tarball or temp tarball. Both requests therefore went to the upstream.
    let package_dir = cache_dir.join(".pnpr-cache").join("foo");
    assert!(
        tarball_cache_entries(&package_dir).is_empty(),
        "a cache:false uplink must not write tarballs to the mirror",
    );

    packument_mock.assert_async().await;
    mock.assert_async().await;
}

#[tokio::test]
async fn cache_false_uplink_rejects_tampered_tarball_without_mirroring() {
    let mut upstream = mockito::Server::new_async().await;
    let good_bytes = b"good-uncached-tarball";
    let poison_bytes = b"poisoned-uncached-tarball";
    let packument_mock =
        mock_packument_for_tarball(&mut upstream, "foo", "1.0.0", good_bytes).await;
    let tarball_mock = upstream
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(poison_bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path().to_path_buf();
    let mut config = config_for(&upstream.url(), cache_dir.clone());
    config.uplinks.get_mut("npmjs").expect("default `npmjs` uplink").cache = false;
    let app = router(config);

    let response = app
        .oneshot(Request::get("/foo/-/foo-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let package_dir = cache_dir.join(".pnpr-cache").join("foo");
    assert!(
        tarball_cache_entries(&package_dir).is_empty(),
        "a rejected cache:false tarball must leave no mirror entry",
    );

    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn resolver_only_serves_resolver_endpoints_and_refuses_registry_routes() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://upstream.invalid", tmp.path().to_path_buf());
    config.registry.enabled = false;
    let app = router(config);

    // The resolver surface stays reachable. `/-/ping` and the capability
    // handshake answer 200; `/-/pnpr/v0/verify-lockfile` is mounted and gated,
    // so an anonymous request is a 401 rather than a 404 (route absent) — that
    // distinction is the point of the assertion.
    let ping =
        app.clone().oneshot(Request::get("/-/ping").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(ping.status(), StatusCode::OK);

    let handshake =
        app.clone().oneshot(Request::get("/-/pnpr").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(handshake.status(), StatusCode::OK);

    let verify = app
        .clone()
        .oneshot(Request::post("/-/pnpr/v0/verify-lockfile").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(verify.status(), StatusCode::UNAUTHORIZED);

    // Every npm-registry route is gone, not merely hidden: a packument
    // read, a publish, and a batch publish all 404 without any upstream
    // call (the route itself is absent).
    let packument =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(packument.status(), StatusCode::NOT_FOUND);

    let publish =
        app.clone().oneshot(Request::put("/foo").body(Body::from("{}")).unwrap()).await.unwrap();
    assert_eq!(publish.status(), StatusCode::NOT_FOUND);

    let batch_publish = app
        .clone()
        .oneshot(Request::put("/-/pnpm/v1/publish").body(Body::from("{}")).unwrap())
        .await
        .unwrap();
    assert_eq!(batch_publish.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn registry_only_serves_registry_and_refuses_resolver_endpoints() {
    let mut upstream = mockito::Server::new_async().await;
    let mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(json!({ "name": "foo", "versions": {} }).to_string())
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.resolver.enabled = false;
    let app = router(config);

    // The registry surface still works: ping and a proxied packument read.
    let ping =
        app.clone().oneshot(Request::get("/-/ping").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(ping.status(), StatusCode::OK);

    let packument =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(packument.status(), StatusCode::OK);

    // The resolver surface is gone: the handshake and both resolver
    // endpoints 404.
    let handshake =
        app.clone().oneshot(Request::get("/-/pnpr").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(handshake.status(), StatusCode::NOT_FOUND);

    // `/-/pnpr` is the only stubbed resolver path, for every method, so
    // capability detection cleanly concludes "no resolver here".
    let handshake_post =
        app.clone().oneshot(Request::post("/-/pnpr").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(handshake_post.status(), StatusCode::NOT_FOUND);

    // `/-/pnpr/v0/resolve` and `/-/pnpr/v0/verify-lockfile` are NOT stubbed:
    // with the resolver disabled they fall through to the registry's
    // four-segment catch-all (`GET|DELETE /{a}/{b}/{c}/{d}`), which has no
    // POST handler, so a POST returns 405. Only `/-/pnpr` needs a stub for
    // clean capability detection.
    let resolve = app
        .clone()
        .oneshot(Request::post("/-/pnpr/v0/resolve").body(Body::from("{}")).unwrap())
        .await
        .unwrap();
    assert_eq!(resolve.status(), StatusCode::METHOD_NOT_ALLOWED);

    let verify = app
        .clone()
        .oneshot(Request::post("/-/pnpr/v0/verify-lockfile").body(Body::from("{}")).unwrap())
        .await
        .unwrap();
    assert_eq!(verify.status(), StatusCode::METHOD_NOT_ALLOWED);

    mock.assert_async().await;
}
