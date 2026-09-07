//! Python resolution through the install accelerator: `POST
//! /-/pnpr/v0/resolve` with `"ecosystem": "pypi"`.
//!
//! The client sends its requirements and the interpreter they are for; the
//! server reads the index and answers with the `pylock.toml` document.
//! These tests stand a mock index up and assert what the server read from
//! it as much as what it returned — above all that it reads a wheel's
//! metadata from the file an index publishes beside it, and downloads the
//! wheel only when there is no such file.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::{AuthState, Config, PublicRoute, router_with_auth};
use serde_json::{Value, json};
use std::{
    io::Write as _,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::PathBuf,
    time::Duration,
};
use tempfile::TempDir;
use tower::ServiceExt;

fn config_for(storage: PathBuf) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::proxy(listen, storage);
    config.public_url = "http://example.test".to_string();
    config.packument_ttl = Duration::from_mins(1);
    config
}

/// The interpreter a request resolves for: `CPython` 3.12 on Linux, taking
/// pure-Python wheels.
fn target() -> Value {
    json!({
        "environment": {
            "implementation_name": "cpython",
            "implementation_version": "3.12.0",
            "os_name": "posix",
            "platform_machine": "x86_64",
            "platform_release": "6.1.0",
            "platform_system": "Linux",
            "platform_version": "#1 SMP",
            "python_full_version": "3.12.0",
            "platform_python_implementation": "CPython",
            "python_version": "3.12",
            "sys_platform": "linux",
        },
        "tags": ["py3-none-any"],
    })
}

fn wheel_bytes(metadata: &str) -> Vec<u8> {
    wheel_of("demo-1.0.0", metadata)
}

fn wheel_of(dist_info: &str, metadata: &str) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    archive
        .start_file::<_, ()>(
            format!("{dist_info}.dist-info/METADATA"),
            zip::write::SimpleFileOptions::default(),
        )
        .expect("start the metadata entry");
    archive.write_all(metadata.as_bytes()).expect("write the metadata entry");
    archive.finish().expect("finish the wheel").into_inner()
}

fn digest(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    format!("{:x}", sha2::Sha256::digest(bytes))
}

fn project_page(files: &Value) -> String {
    json!({ "files": files }).to_string()
}

fn resolve_request(index: &str, token: &str, requirements: &Value) -> Request<Body> {
    let body = json!({
        "ecosystem": "pypi",
        "requirements": requirements,
        "target": target(),
        "index": index,
    });
    Request::post("/-/pnpr/v0/resolve")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

/// Remove the cached document read from a URL ending in `suffix`, which
/// is what its TTL passing would do to it.
fn forget_cached_page(storage: &std::path::Path, suffix: &str) {
    for entry in walkdir::WalkDir::new(storage).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(bytes) = std::fs::read(entry.path()) else { continue };
        let Ok(document) = serde_json::from_slice::<Value>(&bytes) else { continue };
        if document["url"].as_str().is_some_and(|url| url.ends_with(suffix)) {
            std::fs::remove_file(entry.path()).expect("forget the cached page");
        }
    }
}

async fn frames(body: Body) -> Vec<Value> {
    let bytes = to_bytes(body, usize::MAX).await.expect("read body");
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).expect("frame is JSON"))
        .collect()
}

async fn resolved_lockfile(response: axum::response::Response) -> Value {
    assert_eq!(response.status(), StatusCode::OK);
    let frames = frames(response.into_body()).await;
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!(frames[0]["type"], "done", "{frames:?}");
    frames[0]["lockfile"].clone()
}

#[tokio::test]
async fn a_project_resolves_from_the_metadata_files_an_index_publishes() {
    let mut index = mockito::Server::new_async().await;
    let wheel = wheel_bytes("Name: demo\nVersion: 1.0.0\nRequires-Dist: chained >=1\n");
    let chained = wheel_bytes("Name: chained\nVersion: 2.0.0\n");
    let demo_page = index
        .mock("GET", "/simple/demo/")
        .with_body(project_page(&json!([{
            "filename": "demo-1.0.0-py3-none-any.whl",
            "url": "../../files/demo-1.0.0-py3-none-any.whl",
            "hashes": { "sha256": digest(&wheel) },
            "core-metadata": true,
        }])))
        .expect(1)
        .create_async()
        .await;
    let chained_page = index
        .mock("GET", "/simple/chained/")
        .with_body(project_page(&json!([{
            "filename": "chained-2.0.0-py3-none-any.whl",
            "url": "../../files/chained-2.0.0-py3-none-any.whl",
            "hashes": { "sha256": digest(&chained) },
            "core-metadata": true,
        }])))
        .expect(1)
        .create_async()
        .await;
    let demo_metadata = index
        .mock("GET", "/files/demo-1.0.0-py3-none-any.whl.metadata")
        .with_body("Name: demo\nVersion: 1.0.0\nRequires-Dist: chained >=1\n")
        .expect(1)
        .create_async()
        .await;
    let chained_metadata = index
        .mock("GET", "/files/chained-2.0.0-py3-none-any.whl.metadata")
        .with_body("Name: chained\nVersion: 2.0.0\n")
        .expect(1)
        .create_async()
        .await;
    // No wheel is fetched: an index that publishes metadata files spares
    // the server the download resolution would otherwise need.
    let wheels = index
        .mock("GET", mockito::Matcher::Regex(r"^/files/.*\.whl$".to_string()))
        .expect(0)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let mut config = config_for(tmp.path().to_path_buf());
    config.route_policy.public.push(PublicRoute { registry: Some(index.url()), package: None });
    let app = router_with_auth(config, auth);

    let response = app
        .oneshot(resolve_request(&format!("{}/simple/", index.url()), &token, &json!(["demo"])))
        .await
        .unwrap();
    let lockfile = resolved_lockfile(response).await;

    let packages = lockfile["packages"].as_array().expect("the lockfile names packages");
    let mut named = packages
        .iter()
        .map(|package| (package["name"].as_str().unwrap(), package["version"].as_str().unwrap()))
        .collect::<Vec<_>>();
    named.sort_unstable();
    assert_eq!(named, [("chained", "2.0.0"), ("demo", "1.0.0")]);
    assert_eq!(
        packages[0]["wheels"][0]["url"].as_str().unwrap(),
        format!("{}/files/chained-2.0.0-py3-none-any.whl", index.url()),
        "a relative file URL resolves against the page it was read from",
    );
    assert_eq!(lockfile["lock-version"], "1.0");
    for mock in [demo_page, chained_page, demo_metadata, chained_metadata, wheels] {
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn a_wheel_is_read_when_the_index_publishes_no_metadata_file() {
    let mut index = mockito::Server::new_async().await;
    let wheel = wheel_bytes("Name: demo\nVersion: 1.0.0\n");
    let page = index
        .mock("GET", "/simple/demo/")
        .with_body(project_page(&json!([{
            "filename": "demo-1.0.0-py3-none-any.whl",
            "url": "demo-1.0.0-py3-none-any.whl",
            "hashes": { "sha256": digest(&wheel) },
        }])))
        .create_async()
        .await;
    let download = index
        .mock("GET", "/simple/demo/demo-1.0.0-py3-none-any.whl")
        .with_body(&wheel)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let mut config = config_for(tmp.path().to_path_buf());
    config.route_policy.public.push(PublicRoute { registry: Some(index.url()), package: None });
    let app = router_with_auth(config, auth);

    let response = app
        .oneshot(resolve_request(&format!("{}/simple/", index.url()), &token, &json!(["demo"])))
        .await
        .unwrap();
    let lockfile = resolved_lockfile(response).await;

    assert_eq!(lockfile["packages"][0]["name"], "demo");
    page.assert_async().await;
    download.assert_async().await;
}

#[tokio::test]
async fn a_second_resolve_reads_the_cached_index() {
    let mut index = mockito::Server::new_async().await;
    let wheel = wheel_bytes("Name: demo\nVersion: 1.0.0\n");
    let page = index
        .mock("GET", "/simple/demo/")
        .with_body(project_page(&json!([{
            "filename": "demo-1.0.0-py3-none-any.whl",
            "url": "demo-1.0.0-py3-none-any.whl",
            "hashes": { "sha256": digest(&wheel) },
            "core-metadata": true,
        }])))
        .expect(1)
        .create_async()
        .await;
    let metadata = index
        .mock("GET", "/simple/demo/demo-1.0.0-py3-none-any.whl.metadata")
        .with_body("Name: demo\nVersion: 1.0.0\n")
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let mut config = config_for(tmp.path().to_path_buf());
    config.route_policy.public.push(PublicRoute { registry: Some(index.url()), package: None });
    let app = router_with_auth(config, auth);
    let index_url = format!("{}/simple/", index.url());

    let first =
        app.clone().oneshot(resolve_request(&index_url, &token, &json!(["demo"]))).await.unwrap();
    let first = resolved_lockfile(first).await;
    let second = app.oneshot(resolve_request(&index_url, &token, &json!(["demo"]))).await.unwrap();
    let second = resolved_lockfile(second).await;

    assert_eq!(first, second);
    page.assert_async().await;
    metadata.assert_async().await;
}

#[tokio::test]
async fn a_metadata_file_that_is_not_what_the_index_vouched_for_is_refused() {
    let mut index = mockito::Server::new_async().await;
    let wheel = wheel_bytes("Name: demo\nVersion: 1.0.0\n");
    index
        .mock("GET", "/simple/demo/")
        .with_body(project_page(&json!([{
            "filename": "demo-1.0.0-py3-none-any.whl",
            "url": "demo-1.0.0-py3-none-any.whl",
            "hashes": { "sha256": digest(&wheel) },
            "core-metadata": { "sha256": digest(b"Name: demo\nVersion: 1.0.0\n") },
        }])))
        .create_async()
        .await;
    // The metadata file the index actually serves is not the one it
    // published a digest for.
    let metadata = index
        .mock("GET", "/simple/demo/demo-1.0.0-py3-none-any.whl.metadata")
        .with_body("Name: demo\nVersion: 1.0.0\nRequires-Dist: smuggled\n")
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let mut config = config_for(tmp.path().to_path_buf());
    config.route_policy.public.push(PublicRoute { registry: Some(index.url()), package: None });
    let app = router_with_auth(config, auth);

    let response = app
        .oneshot(resolve_request(&format!("{}/simple/", index.url()), &token, &json!(["demo"])))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let frames = frames(response.into_body()).await;
    assert_eq!(frames[0]["type"], "error", "{frames:?}");
    assert!(
        frames[0]["message"]
            .as_str()
            .unwrap()
            .contains("does not match the SHA-256 the index published"),
        "{frames:?}",
    );
    metadata.assert_async().await;
}

#[tokio::test]
async fn metadata_describing_another_distribution_is_refused() {
    let mut index = mockito::Server::new_async().await;
    let wheel = wheel_bytes("Name: demo\nVersion: 1.0.0\n");
    index
        .mock("GET", "/simple/demo/")
        .with_body(project_page(&json!([{
            "filename": "demo-1.0.0-py3-none-any.whl",
            "url": "demo-1.0.0-py3-none-any.whl",
            "hashes": { "sha256": digest(&wheel) },
            "core-metadata": true,
        }])))
        .create_async()
        .await;
    index
        .mock("GET", "/simple/demo/demo-1.0.0-py3-none-any.whl.metadata")
        .with_body("Name: other\nVersion: 9.9.9\n")
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let mut config = config_for(tmp.path().to_path_buf());
    config.route_policy.public.push(PublicRoute { registry: Some(index.url()), package: None });
    let app = router_with_auth(config, auth);

    let response = app
        .oneshot(resolve_request(&format!("{}/simple/", index.url()), &token, &json!(["demo"])))
        .await
        .unwrap();

    let frames = frames(response.into_body()).await;
    assert_eq!(frames[0]["type"], "error", "{frames:?}");
    assert!(
        frames[0]["message"].as_str().unwrap().contains("describes other 9.9.9, not demo 1.0.0"),
        "{frames:?}",
    );
}

#[tokio::test]
async fn a_project_page_that_is_not_one_is_not_cached() {
    let mut index = mockito::Server::new_async().await;
    // Both resolves see the same broken page, so a page cached before it
    // was parsed would leave the second one reading it from disk.
    let page = index
        .mock("GET", "/simple/demo/")
        .with_body("<html>not a Simple API page</html>")
        .expect(2)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let mut config = config_for(tmp.path().to_path_buf());
    config.route_policy.public.push(PublicRoute { registry: Some(index.url()), package: None });
    let app = router_with_auth(config, auth);
    let index_url = format!("{}/simple/", index.url());

    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(resolve_request(&index_url, &token, &json!(["demo"])))
            .await
            .unwrap();
        let frames = frames(response.into_body()).await;
        assert_eq!(frames[0]["type"], "error", "{frames:?}");
    }

    page.assert_async().await;
}

#[tokio::test]
async fn an_index_url_keeps_its_query_on_every_read() {
    let mut index = mockito::Server::new_async().await;
    let wheel = wheel_bytes("Name: demo\nVersion: 1.0.0\n");
    let page = index
        .mock("GET", "/simple/demo/")
        .match_query(mockito::Matcher::UrlEncoded("token".to_string(), "secret".to_string()))
        .with_body(project_page(&json!([{
            "filename": "demo-1.0.0-py3-none-any.whl",
            "url": "demo-1.0.0-py3-none-any.whl?token=secret",
            "hashes": { "sha256": digest(&wheel) },
            "core-metadata": true,
        }])))
        .expect(1)
        .create_async()
        .await;
    let metadata = index
        .mock("GET", "/simple/demo/demo-1.0.0-py3-none-any.whl.metadata")
        .match_query(mockito::Matcher::UrlEncoded("token".to_string(), "secret".to_string()))
        .with_body("Name: demo\nVersion: 1.0.0\n")
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let mut config = config_for(tmp.path().to_path_buf());
    config.route_policy.public.push(PublicRoute { registry: Some(index.url()), package: None });
    let app = router_with_auth(config, auth);

    let response = app
        .oneshot(resolve_request(
            &format!("{}/simple/?token=secret", index.url()),
            &token,
            &json!(["demo"]),
        ))
        .await
        .unwrap();
    let lockfile = resolved_lockfile(response).await;

    assert_eq!(lockfile["packages"][0]["name"], "demo");
    page.assert_async().await;
    metadata.assert_async().await;
}

#[tokio::test]
async fn cached_metadata_is_refused_once_the_index_publishes_another_digest() {
    let mut index = mockito::Server::new_async().await;
    let wheel = wheel_bytes("Name: demo\nVersion: 1.0.0\n");
    let served = "Name: demo\nVersion: 1.0.0\n";
    let page = |digest_of: &str| {
        project_page(&json!([{
            "filename": "demo-1.0.0-py3-none-any.whl",
            "url": "demo-1.0.0-py3-none-any.whl",
            "hashes": { "sha256": digest(&wheel) },
            "core-metadata": { "sha256": digest(digest_of.as_bytes()) },
        }]))
    };
    let first_page =
        index.mock("GET", "/simple/demo/").with_body(page(served)).expect(1).create_async().await;
    let metadata = index
        .mock("GET", "/simple/demo/demo-1.0.0-py3-none-any.whl.metadata")
        .with_body(served)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let mut config = config_for(tmp.path().to_path_buf());
    config.route_policy.public.push(PublicRoute { registry: Some(index.url()), package: None });
    let app = router_with_auth(config, auth);
    let index_url = format!("{}/simple/", index.url());

    resolved_lockfile(
        app.clone().oneshot(resolve_request(&index_url, &token, &json!(["demo"]))).await.unwrap(),
    )
    .await;
    first_page.assert_async().await;
    metadata.assert_async().await;

    // Drop the cached project page, as its TTL passing would: the metadata
    // entry outlives it, because it was written when a resolve first needed
    // that version rather than when the page was read. The index now
    // vouches for a different metadata file than the cache holds.
    forget_cached_page(tmp.path(), "/simple/demo/");
    index
        .mock("GET", "/simple/demo/")
        .with_body(page("Name: demo\nVersion: 1.0.0\nRequires-Dist: added\n"))
        .create_async()
        .await;
    let response =
        app.oneshot(resolve_request(&index_url, &token, &json!(["demo"]))).await.unwrap();

    let frames = frames(response.into_body()).await;
    assert_eq!(frames[0]["type"], "error", "{frames:?}");
    assert!(
        frames[0]["message"].as_str().unwrap().contains("does not match the SHA-256"),
        "{frames:?}",
    );
}

/// An index that publishes no metadata file leaves the server nothing to
/// re-check an extracted document against, so the wheel it came out of has
/// to be the thing the cache entry is bound to.
#[tokio::test]
async fn metadata_read_from_a_wheel_is_not_reused_for_the_wheel_that_replaces_it() {
    let mut index = mockito::Server::new_async().await;
    let first = wheel_bytes("Name: demo\nVersion: 1.0.0\n");
    let second = wheel_bytes("Name: demo\nVersion: 1.0.0\nRequires-Dist: later\n");
    let later = wheel_of("later-1.0.0", "Name: later\nVersion: 1.0.0\n");
    let page = |wheel: &[u8], filename: &str| {
        project_page(&json!([{
            "filename": filename,
            "url": filename,
            "hashes": { "sha256": digest(wheel) },
        }]))
    };
    let demo_page = |wheel: &[u8]| page(wheel, "demo-1.0.0-py3-none-any.whl");
    let first_page = index
        .mock("GET", "/simple/demo/")
        .with_body(demo_page(&first))
        .expect(1)
        .create_async()
        .await;
    let first_wheel = index
        .mock("GET", "/simple/demo/demo-1.0.0-py3-none-any.whl")
        .with_body(&first)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let mut config = config_for(tmp.path().to_path_buf());
    config.route_policy.public.push(PublicRoute { registry: Some(index.url()), package: None });
    let app = router_with_auth(config, auth);
    let index_url = format!("{}/simple/", index.url());

    let locked = resolved_lockfile(
        app.clone().oneshot(resolve_request(&index_url, &token, &json!(["demo"]))).await.unwrap(),
    )
    .await;
    assert_eq!(locked["packages"].as_array().unwrap().len(), 1, "{locked}");
    first_page.assert_async().await;
    first_wheel.assert_async().await;

    // Drop the cached project page, as its TTL passing would. The index now
    // serves another wheel at the address the metadata was read from.
    forget_cached_page(tmp.path(), "/simple/demo/");
    index.mock("GET", "/simple/demo/").with_body(demo_page(&second)).create_async().await;
    index
        .mock("GET", "/simple/demo/demo-1.0.0-py3-none-any.whl")
        .with_body(&second)
        .create_async()
        .await;
    index
        .mock("GET", "/simple/later/")
        .with_body(page(&later, "later-1.0.0-py3-none-any.whl"))
        .create_async()
        .await;
    index
        .mock("GET", "/simple/later/later-1.0.0-py3-none-any.whl")
        .with_body(&later)
        .create_async()
        .await;

    let locked = resolved_lockfile(
        app.oneshot(resolve_request(&index_url, &token, &json!(["demo"]))).await.unwrap(),
    )
    .await;
    let locked = locked["packages"].as_array().unwrap();
    let names = locked.iter().map(|package| package["name"].as_str().unwrap()).collect::<Vec<_>>();
    assert_eq!(names, ["demo", "later"], "{locked:?}");
}

#[tokio::test]
async fn an_unsatisfiable_project_is_reported_as_one() {
    let mut index = mockito::Server::new_async().await;
    let wheel = wheel_bytes("Name: demo\nVersion: 1.0.0\n");
    index
        .mock("GET", "/simple/demo/")
        .with_body(project_page(&json!([{
            "filename": "demo-1.0.0-py3-none-any.whl",
            "url": "demo-1.0.0-py3-none-any.whl",
            "hashes": { "sha256": digest(&wheel) },
            "core-metadata": true,
        }])))
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let mut config = config_for(tmp.path().to_path_buf());
    config.route_policy.public.push(PublicRoute { registry: Some(index.url()), package: None });
    let app = router_with_auth(config, auth);

    let response = app
        .oneshot(resolve_request(&format!("{}/simple/", index.url()), &token, &json!(["demo>=2"])))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let frames = frames(response.into_body()).await;
    assert_eq!(frames[0]["type"], "error", "{frames:?}");
    assert!(frames[0]["message"].as_str().unwrap().contains("resolution failed"), "{frames:?}");
}

#[tokio::test]
async fn an_off_allowlist_index_is_refused() {
    let index = mockito::Server::new_async().await;
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config_for(tmp.path().to_path_buf()), auth);

    let response = app
        .oneshot(resolve_request(&format!("{}/simple/", index.url()), &token, &json!(["demo"])))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_requirement_naming_a_url_is_refused() {
    let index = mockito::Server::new_async().await;
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let mut config = config_for(tmp.path().to_path_buf());
    config.route_policy.public.push(PublicRoute { registry: Some(index.url()), package: None });
    let app = router_with_auth(config, auth);

    let response = app
        .oneshot(resolve_request(
            &format!("{}/simple/", index.url()),
            &token,
            &json!(["demo @ https://example.test/demo-1.0.0-py3-none-any.whl"]),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.expect("read body");
    assert!(
        String::from_utf8_lossy(&bytes).contains("direct URL"),
        "{}",
        String::from_utf8_lossy(&bytes),
    );
}
