//! The Python package index surface: the legacy upload API, the Simple API
//! in both renderings, file downloads, and proxying an upstream index.

// `#[path]` rather than the `tests/common/mod.rs` layout, which the
// Perfectionist dylint forbids.
#[path = "common/ecosystem.rs"]
mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use common::{HostedSource, PUBLIC_URL, body_bytes, find_file, mixed_router_config, sha256_hex};
use pnpr::{AuthState, Config, Ecosystem, router_with_auth};
use serde_json::{Value, json};
use std::path::PathBuf;
use tempfile::TempDir;
use tower::ServiceExt;

const JSON: &str = "application/vnd.pypi.simple.v1+json";
const BOUNDARY: &str = "pnprTestBoundary";

/// A hosted Python registry (`internal`, claiming `demo-pkg`) and a Python
/// upstream (`pypiorg`, everything else) at `upstream_url`, both in the
/// `main` router beside the npm registries.
fn pypi_config(storage: PathBuf, upstream_url: &str) -> Config {
    mixed_router_config(
        storage,
        Ecosystem::Pypi,
        HostedSource { name: "internal", org: "python", access: "$all", packages: &["demo-pkg"] },
        ("pypiorg", upstream_url),
    )
}

/// The `multipart/form-data` body `twine upload` sends, with the fields the
/// server acts on plus a sample of the metadata it ignores.
fn upload_form(fields: &[(&str, &str)], filename: &str, content: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n",
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"content\"; \
             filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n",
        )
        .as_bytes(),
    );
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    body
}

fn wheel_upload(name: &str, version: &str, filename: &str, content: &[u8]) -> Vec<u8> {
    let digest = sha256_hex(content);
    upload_form(
        &[
            (":action", "file_upload"),
            ("protocol_version", "1"),
            ("metadata_version", "2.1"),
            ("name", name),
            ("version", version),
            ("filetype", "bdist_wheel"),
            ("pyversion", "py3"),
            ("summary", "A demo"),
            ("requires_python", ">=3.9"),
            ("sha256_digest", &digest),
        ],
        filename,
        content,
    )
}

/// `twine` sends the token as `Basic __token__:<token>`.
fn upload_request(token: Option<&str>, body: Vec<u8>) -> Request<Body> {
    let mut request = Request::post("/pypi/legacy/")
        .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={BOUNDARY}"));
    if let Some(token) = token {
        let credentials = BASE64_STANDARD.encode(format!("__token__:{token}"));
        request = request.header(header::AUTHORIZATION, format!("Basic {credentials}"));
    }
    request.body(Body::from(body)).unwrap()
}

fn get(path: &str, accept: Option<&str>) -> Request<Body> {
    let mut request = Request::get(path);
    if let Some(accept) = accept {
        request = request.header(header::ACCEPT, accept);
    }
    request.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn uploads_a_wheel_and_serves_the_simple_pages_and_the_file() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app =
        router_with_auth(pypi_config(tmp.path().to_path_buf(), "http://upstream.invalid/"), auth);
    let wheel = b"PK\x03\x04 pretend wheel".to_vec();
    let filename = "demo_pkg-1.0.0-py3-none-any.whl";

    let response = app
        .clone()
        .oneshot(upload_request(Some(&token), wheel_upload("Demo_Pkg", "1.0.0", filename, &wheel)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(tmp.path().join("python/demo-pkg").join(filename).is_file());

    // PEP 691 JSON, with the file URL pointing back at this registry.
    let response = app.clone().oneshot(get("/pypi/simple/demo-pkg/", Some(JSON))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], JSON);
    // A public project through the default target stays cacheable.
    assert!(response.headers().get(header::CACHE_CONTROL).is_none());
    let page: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(page["meta"]["api-version"], "1.1");
    assert_eq!(page["name"], "demo-pkg");
    assert_eq!(page["versions"], json!(["1.0.0"]));
    let file = &page["files"][0];
    assert_eq!(file["filename"], filename);
    assert_eq!(file["url"], format!("{PUBLIC_URL}/pypi/files/demo-pkg/{filename}"));
    assert_eq!(file["hashes"]["sha256"], sha256_hex(&wheel));
    assert_eq!(file["requires-python"], ">=3.9");
    assert_eq!(file["yanked"], false);
    assert_eq!(file["size"], wheel.len());
    assert!(file["upload-time"].is_string());

    // PEP 503 HTML for clients that do not ask for JSON.
    let response = app.clone().oneshot(get("/pypi/simple/demo-pkg/", None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "text/html; charset=utf-8");
    let html = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    let expected_anchor = format!(
        concat!(
            r#"<a href="{PUBLIC_URL}/pypi/files/demo-pkg/{filename}#sha256={digest}""#,
            r#" data-requires-python="&gt;=3.9">{filename}</a>"#,
        ),
        PUBLIC_URL = PUBLIC_URL,
        filename = filename,
        digest = sha256_hex(&wheel),
    );
    assert!(html.contains(&expected_anchor), "{html}");

    // The trailing-slash-less form and the project list.
    let response = app.clone().oneshot(get("/pypi/simple/demo-pkg", Some(JSON))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = app.clone().oneshot(get("/pypi/simple/", Some(JSON))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let list: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(list["projects"], json!([{ "name": "demo-pkg" }]));
    let response = app.clone().oneshot(get("/pypi/simple", None)).await.unwrap();
    let html = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(
        html.contains(&format!(r#"<a href="{PUBLIC_URL}/pypi/simple/demo-pkg/">demo-pkg</a>"#)),
        "{html}",
    );

    // A non-normalized spelling redirects to the canonical page.
    let response = app.clone().oneshot(get("/pypi/simple/Demo_Pkg/", None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
    assert_eq!(response.headers()[header::LOCATION], format!("{PUBLIC_URL}/pypi/simple/demo-pkg/"));

    // The file itself.
    let response =
        app.clone().oneshot(get(&format!("/pypi/files/demo-pkg/{filename}"), None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, wheel);

    // The named form is caller-scoped and points file URLs at itself.
    let response =
        app.clone().oneshot(get("/pypi/~internal/simple/demo-pkg/", Some(JSON))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");
    let page: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(
        page["files"][0]["url"],
        format!("{PUBLIC_URL}/pypi/~internal/files/demo-pkg/{filename}"),
    );
    // npm-shaped paths mean nothing on the Python surface, and the old
    // npm-only address of a registry has no Simple API.
    let response = app.clone().oneshot(get("/pypi/demo-pkg", None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = app.oneshot(get("/~internal/simple/demo-pkg/", None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn upload_is_authenticated_and_validated() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app =
        router_with_auth(pypi_config(tmp.path().to_path_buf(), "http://upstream.invalid/"), auth);
    let wheel = b"wheel bytes".to_vec();
    let filename = "demo_pkg-1.0.0-py3-none-any.whl";
    let good = wheel_upload("demo-pkg", "1.0.0", filename, &wheel);

    let response = app.clone().oneshot(upload_request(None, good.clone())).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app.clone().oneshot(upload_request(Some(&token), good.clone())).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // The same filename cannot be uploaded twice.
    let response = app.clone().oneshot(upload_request(Some(&token), good)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let text = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(text.contains("File already exists"), "{text}");

    // The filename must belong to the declared project and version.
    let response = app
        .clone()
        .oneshot(upload_request(
            Some(&token),
            wheel_upload("demo-pkg", "1.0.0", "other-1.0.0-py3-none-any.whl", &wheel),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = app
        .clone()
        .oneshot(upload_request(
            Some(&token),
            wheel_upload("demo-pkg", "2.0.0", "demo_pkg-1.0.0-py3-none-any.whl", &wheel),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // A declared digest must match the bytes.
    let mut lying = upload_form(
        &[
            (":action", "file_upload"),
            ("protocol_version", "1"),
            ("name", "demo-pkg"),
            ("version", "1.1.0"),
            ("filetype", "bdist_wheel"),
            ("sha256_digest", &sha256_hex(b"not these bytes")),
        ],
        "demo_pkg-1.1.0-py3-none-any.whl",
        &wheel,
    );
    let response = app.clone().oneshot(upload_request(Some(&token), lying.clone())).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    lying.clear();

    // The filetype must match the filename.
    let sdist_as_wheel = upload_form(
        &[
            (":action", "file_upload"),
            ("protocol_version", "1"),
            ("name", "demo-pkg"),
            ("version", "1.1.0"),
            ("filetype", "bdist_wheel"),
        ],
        "demo_pkg-1.1.0.tar.gz",
        &wheel,
    );
    let response = app.clone().oneshot(upload_request(Some(&token), sdist_as_wheel)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // A project the hosted registry does not claim routes to the upstream,
    // where nothing can be uploaded.
    let response = app
        .clone()
        .oneshot(upload_request(
            Some(&token),
            wheel_upload("requests", "1.0.0", "requests-1.0.0-py3-none-any.whl", &wheel),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let text = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(text.contains("upstream registry"), "{text}");

    // Only the one wheel landed.
    let response = app.oneshot(get("/pypi/simple/demo-pkg/", Some(JSON))).await.unwrap();
    let page: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(page["files"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn proxies_a_simple_page_and_verified_downloads_through_an_upstream() {
    let mut upstream = mockito::Server::new_async().await;
    let wheel = b"requests wheel bytes".to_vec();
    let filename = "requests-2.32.0-py3-none-any.whl";
    let page_mock = upstream
        .mock("GET", "/simple/requests/")
        .match_header(
            header::ACCEPT.as_str(),
            mockito::Matcher::Regex(r"vnd.pypi.simple.v1\+json".to_string()),
        )
        .with_header(header::CONTENT_TYPE.as_str(), JSON)
        .with_body(
            json!({
                "meta": { "api-version": "1.1", "_last-serial": 42 },
                "name": "requests",
                "versions": ["2.32.0"],
                "files": [{
                    "filename": filename,
                    // Relative to the page URL, as pypi.org serves them.
                    "url": format!("../../packages/ab/cd/{filename}"),
                    "hashes": { "sha256": sha256_hex(&wheel) },
                    "requires-python": ">=3.8",
                    "yanked": false,
                    "size": wheel.len(),
                    "upload-time": "2026-01-01T00:00:00.000000Z",
                    "core-metadata": { "sha256": "irrelevant" },
                }],
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let file_mock = upstream
        .mock("GET", format!("/packages/ab/cd/{filename}").as_str())
        .with_body(wheel.clone())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router_with_auth(
        pypi_config(tmp.path().to_path_buf(), &format!("{}/simple/", upstream.url())),
        AuthState::in_memory(),
    );

    // The page is re-rendered with file URLs pointing back at this registry.
    let response = app.clone().oneshot(get("/pypi/simple/requests/", Some(JSON))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let page: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(page["name"], "requests");
    assert_eq!(page["files"][0]["url"], format!("{PUBLIC_URL}/pypi/files/requests/{filename}"));
    assert_eq!(page["files"][0]["hashes"]["sha256"], sha256_hex(&wheel));
    assert_eq!(page["files"][0]["requires-python"], ">=3.8");

    // The HTML form is rendered from the same cached page.
    let response = app.clone().oneshot(get("/pypi/simple/requests/", None)).await.unwrap();
    let html = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert!(html.contains(&format!("/pypi/files/requests/{filename}#sha256=")), "{html}");

    // The file is fetched from the page's URL, verified, and cached.
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(get(&format!("/pypi/files/requests/{filename}"), None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response.into_body()).await, wheel);
    }
    page_mock.assert_async().await;
    file_mock.assert_async().await;
    assert!(find_file(&tmp.path().join(".pnpr-cache"), filename).is_some(), "file is cached");

    // A file the page does not list is never fetched.
    let response = app
        .clone()
        .oneshot(get("/pypi/files/requests/requests-9.9.9-py3-none-any.whl", None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // An unknown project is a definitive 404.
    let missing = upstream.mock("GET", "/simple/nope/").with_status(404).create_async().await;
    let response = app.oneshot(get("/pypi/simple/nope/", Some(JSON))).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    missing.assert_async().await;
}

#[tokio::test]
async fn an_upstream_without_the_json_api_is_a_gateway_error_and_a_bad_hash_is_never_cached() {
    let mut upstream = mockito::Server::new_async().await;
    upstream
        .mock("GET", "/simple/html-only/")
        .with_header(header::CONTENT_TYPE.as_str(), "text/html")
        .with_body(r#"<html><a href="x.whl">x.whl</a></html>"#)
        .create_async()
        .await;
    let filename = "lying-1.0.0-py3-none-any.whl";
    upstream
        .mock("GET", "/simple/lying/")
        .with_body(
            json!({
                "name": "lying",
                "files": [{
                    "filename": filename,
                    "url": format!("{}/files/{filename}", upstream.url()),
                    "hashes": { "sha256": sha256_hex(b"other bytes") },
                }],
            })
            .to_string(),
        )
        .create_async()
        .await;
    upstream
        .mock("GET", format!("/files/{filename}").as_str())
        .with_body("real bytes")
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router_with_auth(
        pypi_config(tmp.path().to_path_buf(), &format!("{}/simple/", upstream.url())),
        AuthState::in_memory(),
    );

    let response = app.clone().oneshot(get("/pypi/simple/html-only/", Some(JSON))).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

    let response = app.oneshot(get(&format!("/pypi/files/lying/{filename}"), None)).await.unwrap();
    let _ = body_bytes(response.into_body()).await;
    assert!(find_file(&tmp.path().join(".pnpr-cache"), filename).is_none());
}

#[tokio::test]
async fn cached_files_follow_current_page_checksums_and_removals() {
    common::assert_cache_tracks_metadata(Ecosystem::Pypi).await;
}

#[tokio::test]
async fn anonymous_uploads_are_rejected_before_reading_the_body() {
    let tmp = TempDir::new().unwrap();
    let app = router_with_auth(
        pypi_config(tmp.path().to_path_buf(), "http://upstream.invalid/"),
        AuthState::in_memory(),
    );
    for path in ["/pypi/legacy", "/pypi/legacy/", "/pypi/~internal/legacy/"] {
        let body = Body::from_stream(futures_util::stream::poll_fn(
            |_| -> std::task::Poll<Option<Result<axum::body::Bytes, std::io::Error>>> {
                panic!("anonymous upload body must not be polled");
            },
        ));
        let response = app.clone().oneshot(Request::post(path).body(body).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn conflicting_object_store_upload_does_not_publish_metadata_or_leave_staged_bytes() {
    use object_store::{ObjectStoreExt, memory::InMemory, path::Path as ObjectPath};
    use pnpr::HostedStoreConfig;
    use std::sync::Arc;

    let tmp = TempDir::new().unwrap();
    let store = Arc::new(InMemory::new());
    let filename = "demo_pkg-1.0.0-py3-none-any.whl";
    let object = ObjectPath::from(format!("python/demo-pkg/{filename}"));
    store.put(&object, axum::body::Bytes::from_static(b"winning artifact").into()).await.unwrap();
    let mut config = pypi_config(tmp.path().to_path_buf(), "http://upstream.invalid/");
    config.hosted_store = HostedStoreConfig::ObjectStore {
        store: Arc::<InMemory>::clone(&store),
        prefix: String::new(),
    };
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config, auth);
    let response = app
        .clone()
        .oneshot(upload_request(
            Some(&token),
            wheel_upload("demo-pkg", "1.0.0", filename, b"losing artifact"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(store.get(&object).await.unwrap().bytes().await.unwrap(), "winning artifact");
    assert_eq!(
        app.oneshot(get("/pypi/simple/demo-pkg/", Some(JSON))).await.unwrap().status(),
        StatusCode::NOT_FOUND,
    );
    fn assert_no_staged_files(path: &std::path::Path) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                assert_no_staged_files(&path);
            } else {
                assert!(!path.to_string_lossy().contains(".tmp"), "{}", path.display());
            }
        }
    }
    assert_no_staged_files(tmp.path());
}

#[tokio::test]
async fn upstream_file_hosts_must_be_approved_by_the_operator() {
    use pnpr::PublicRoute;
    let mut upstream = mockito::Server::new_async().await;
    let mut files = mockito::Server::new_async().await;
    let filename = "requests-1.0.0-py3-none-any.whl";
    let bytes = b"wheel bytes";
    let page = upstream
        .mock("GET", "/requests/")
        .with_body(
            json!({
                "meta": { "api-version": "1.0" }, "name": "requests", "files": [{
                    "filename": filename, "url": format!("{}/artifact", files.url()),
                    "hashes": { "sha256": sha256_hex(bytes) },
                }],
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let artifact = files.mock("GET", "/artifact").with_body(bytes).expect(0).create_async().await;
    let tmp = TempDir::new().unwrap();
    let mut config = pypi_config(tmp.path().to_path_buf(), &upstream.url());
    let app = router_with_auth(config.clone(), AuthState::in_memory());
    let response =
        app.oneshot(get(&format!("/pypi/files/requests/{filename}"), None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    artifact.assert_async().await;
    artifact.remove_async().await;

    config.route_policy.public.push(PublicRoute { registry: Some(files.url()), package: None });
    let app = router_with_auth(config, AuthState::in_memory());
    let artifact = files.mock("GET", "/artifact").with_body(bytes).expect(1).create_async().await;
    let response =
        app.oneshot(get(&format!("/pypi/files/requests/{filename}"), None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, bytes);
    artifact.assert_async().await;
    page.assert_async().await;
}

#[tokio::test]
async fn hosted_downloads_reject_files_absent_from_publication_metadata() {
    let tmp = TempDir::new().unwrap();
    let package_dir = tmp.path().join("python/demo-pkg");
    tokio::fs::create_dir_all(&package_dir).await.unwrap();
    let orphan = "demo_pkg-1.0.0-py3-none-any.whl";
    tokio::fs::write(package_dir.join(orphan), b"unpublished wheel").await.unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app =
        router_with_auth(pypi_config(tmp.path().to_path_buf(), "http://upstream.invalid/"), auth);
    let orphan_path = format!("/pypi/files/demo-pkg/{orphan}");
    let response = app.clone().oneshot(get(&orphan_path, None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let published = "demo_pkg-2.0.0-py3-none-any.whl";
    let response = app
        .clone()
        .oneshot(upload_request(
            Some(&token),
            wheel_upload("demo-pkg", "2.0.0", published, b"published wheel"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = app.clone().oneshot(get(&orphan_path, None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response =
        app.oneshot(get(&format!("/pypi/files/demo-pkg/{published}"), None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, b"published wheel");
}
