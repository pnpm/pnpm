//! A version the fresh cached packument does not list: the routes that serve
//! one version refetch the packument once it is old enough, so a version
//! published upstream after the fetch is served before the packument expires.

use super::{
    Body,
    Duration,
    Request,
    ServiceExt,
    StatusCode,
    TempDir,
    body_bytes,
    config_for,
    json,
    router,
    sha512_integrity,
};
use std::{
    path::Path,
    time::SystemTime,
};

const TTL: Duration = Duration::from_hours(24);

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

fn packument(upstream: &str, versions: &[(&str, &[u8])]) -> String {
    let manifests: serde_json::Map<String, serde_json::Value> = versions
        .iter()
        .map(|(version, tgz)| {
            let manifest = json!({ "name": "foo", "version": version, "dist": {
                "tarball": format!("{upstream}/foo/-/foo-{version}.tgz"),
                "integrity": sha512_integrity(tgz),
            } });
            (version.to_string(), manifest)
        })
        .collect();
    let latest = versions.last().unwrap().0;
    json!({ "name": "foo", "dist-tags": { "latest": latest }, "versions": manifests }).to_string()
}

fn app(upstream: &str, storage: &Path) -> axum::Router {
    let mut config = config_for(upstream, storage.to_path_buf());
    config.routing.upstreams.get_mut("npmjs").unwrap().packument_ttl = Some(TTL);
    router(config)
}

/// Date every file under `dir` back by `age`, so each cached packument reads
/// as fetched that long ago and stays within [`TTL`].
fn age_cached_packuments(dir: &Path, age: Duration) {
    let fetched = SystemTime::now() - age;
    let files = walkdir::WalkDir::new(dir)
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().is_file());
    for entry in files {
        let path = entry.path();
        if path
            .file_name()
            .is_some_and(|name| name == "fetched-at.json")
        {
            std::fs::write(path, serde_json::to_vec(&fetched).unwrap()).unwrap();
        }
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(fetched)
            .unwrap();
    }
}

/// Cache `foo`'s packument listing only 1.0.0.
async fn cache_first_version(upstream: &mut mockito::ServerGuard, app: &axum::Router) {
    let cached = upstream
        .mock("GET", "/foo")
        .with_header("content-type", "application/json")
        .with_body(packument(&upstream.url(), &[("1.0.0", b"old")]))
        .expect(1)
        .create_async()
        .await;
    let response = get(app, "/npm/foo").await;
    assert_eq!(response.status(), StatusCode::OK);
    cached.assert_async().await;
    cached.remove_async().await;
}

/// Cache `foo`'s packument listing only 1.0.0, then publish 1.0.1 upstream.
/// Returns the mock that serves the packument with both versions.
async fn publish_after_caching(
    upstream: &mut mockito::ServerGuard,
    app: &axum::Router,
    new_tgz: &[u8],
) -> mockito::Mock {
    cache_first_version(upstream, app).await;
    upstream
        .mock("GET", "/foo/-/foo-1.0.1.tgz")
        .with_body(new_tgz)
        .create_async()
        .await;
    upstream
        .mock("GET", "/foo")
        .with_header("content-type", "application/json")
        .with_body(packument(&upstream.url(), &[("1.0.0", b"old"), ("1.0.1", new_tgz)]))
        .expect(1)
        .create_async()
        .await
}

/// Cache `foo`'s packument listing only 1.0.0, date it back by `age`, and
/// make the upstream fail. Returns the mock that fails.
async fn fail_after_caching(
    upstream: &mut mockito::ServerGuard,
    app: &axum::Router,
    storage: &Path,
    age: Duration,
) -> mockito::Mock {
    cache_first_version(upstream, app).await;
    age_cached_packuments(storage, age);
    upstream
        .mock("GET", "/foo")
        .with_status(500)
        .expect(1)
        .create_async()
        .await
}

#[tokio::test]
async fn a_version_published_after_the_packument_was_cached_is_served() {
    for route in ["/npm/foo@1.0.1", "/npm/foo/-/foo-1.0.1.tgz"] {
        let new_tgz: &[u8] = b"new";
        let mut upstream = mockito::Server::new_async().await;
        let tmp = TempDir::new().unwrap();
        let app = app(&upstream.url(), tmp.path());
        let refetched = publish_after_caching(&mut upstream, &app, new_tgz).await;
        age_cached_packuments(tmp.path(), Duration::from_mins(2));

        let response = get(&app, route).await;
        assert_eq!(response.status(), StatusCode::OK, "{route}");
        assert_eq!(body_bytes(response.into_body()).await, new_tgz, "{route}");
        let listed = get(&app, "/npm/foo").await;
        let listed: serde_json::Value =
            serde_json::from_slice(&body_bytes(listed.into_body()).await).unwrap();
        assert!(listed["versions"].get("1.0.1").is_some(), "{route} refreshed the cache");
        refetched.assert_async().await;
    }
}

#[tokio::test]
async fn a_whole_packument_request_can_revalidate_cached_metadata() {
    let mut upstream = mockito::Server::new_async().await;
    let tmp = TempDir::new().unwrap();
    let app = app(&upstream.url(), tmp.path());
    let refetched = publish_after_caching(&mut upstream, &app, b"new").await;
    let response = app
        .oneshot(
            Request::get("/npm/foo")
                .header("cache-control", "no-cache")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let listed: serde_json::Value =
        serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert!(listed["versions"].get("1.0.1").is_some());
    refetched.assert_async().await;
}

#[tokio::test]
async fn a_version_manifest_published_after_the_packument_was_cached_is_served() {
    for version_or_tag in ["1.0.1", "latest"] {
        let mut upstream = mockito::Server::new_async().await;
        let tmp = TempDir::new().unwrap();
        let app = app(&upstream.url(), tmp.path());
        let refetched = publish_after_caching(&mut upstream, &app, b"new").await;
        age_cached_packuments(tmp.path(), Duration::from_mins(2));

        let uri = format!("/npm/foo/{version_or_tag}");
        let response = get(&app, &uri).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        let manifest: serde_json::Value =
            serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        let expected = if version_or_tag == "latest" { "1.0.0" } else { "1.0.1" };
        assert_eq!(manifest["version"], expected, "{uri}");
        assert_eq!(refetched.matched_async().await, version_or_tag == "1.0.1", "{uri}");
    }
}

#[tokio::test]
async fn a_recently_fetched_packument_is_not_refetched_for_an_unlisted_version() {
    let mut upstream = mockito::Server::new_async().await;
    let tmp = TempDir::new().unwrap();
    let app = app(&upstream.url(), tmp.path());
    let refetched = publish_after_caching(&mut upstream, &app, b"new").await;

    for route in ["/npm/foo@1.0.1", "/npm/foo/-/foo-1.0.1.tgz", "/npm/foo/1.0.1"] {
        let response = get(&app, route).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{route}");
    }
    assert!(!refetched.matched_async().await);
}

#[tokio::test]
async fn an_unlisted_version_refetches_an_old_packument_once() {
    let mut upstream = mockito::Server::new_async().await;
    let tmp = TempDir::new().unwrap();
    let app = app(&upstream.url(), tmp.path());
    let refetched = publish_after_caching(&mut upstream, &app, b"new").await;
    age_cached_packuments(tmp.path(), Duration::from_mins(2));

    for route in ["/npm/foo@9.9.9", "/npm/foo/-/foo-9.9.9.tgz", "/npm/foo/9.9.9"] {
        let response = get(&app, route).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{route}");
    }
    refetched.assert_async().await;
}

#[tokio::test]
async fn a_transient_refetch_error_can_be_retried_immediately() {
    let mut upstream = mockito::Server::new_async().await;
    let tmp = TempDir::new().unwrap();
    let app = app(&upstream.url(), tmp.path());
    let failing = fail_after_caching(&mut upstream, &app, tmp.path(), Duration::from_mins(2)).await;

    let response = get(&app, "/npm/foo/1.0.1").await;
    assert!(response.status().is_server_error());
    failing.assert_async().await;
    failing.remove_async().await;
    let recovered = upstream
        .mock("GET", "/foo")
        .with_body(packument(&upstream.url(), &[("1.0.1", b"new")]))
        .expect(1)
        .create_async()
        .await;
    assert_eq!(get(&app, "/npm/foo/1.0.1").await.status(), StatusCode::OK);
    recovered.assert_async().await;
}

/// The read of an expired packument already asked the upstream.
#[tokio::test]
async fn an_expired_packument_is_not_refetched_twice() {
    let mut upstream = mockito::Server::new_async().await;
    let tmp = TempDir::new().unwrap();
    let app = app(&upstream.url(), tmp.path());
    let failing = fail_after_caching(&mut upstream, &app, tmp.path(), TTL * 2).await;

    let response = get(&app, "/npm/foo@1.0.1").await;
    assert!(response.status().is_server_error());
    failing.assert_async().await;
}
