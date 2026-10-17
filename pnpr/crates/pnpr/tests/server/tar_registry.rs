use super::{
    AuthState, Body, Config, Ecosystem, PackagePattern, Registries, Registry, Request, ServiceExt,
    StatusCode, TempDir, body_bytes, body_json, config_for, header, hosted_publish_request,
    hosted_with_access, router, router_with_auth, sha512_integrity,
};
use crate::tokens::issue_token;
use std::io::Write;

fn tar_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, body) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, path, *body).unwrap();
    }
    builder.into_inner().unwrap()
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

fn widget_tar() -> Vec<u8> {
    tar_of(&[("package/package.json", br#"{"name":"@acme/widget","version":"1.0.0"}"#)])
}

/// A hosted `acme` registry behind two routers: `main` serving `.tgz` and the
/// path-less default, `zstd` serving tars.
fn acme_config(storage: std::path::PathBuf) -> Config {
    let mut config = config_for("http://127.0.0.1:1", storage);
    config.routing.hosted.insert("acme".to_string(), hosted_with_access("acme", "$all"));
    let graph = vec![
        ("npmjs".to_string(), Registry::Upstream { patterns: vec![] }),
        (
            "acme".to_string(),
            Registry::Hosted {
                patterns: vec![PackagePattern::parse("@acme/*", Ecosystem::Npm).unwrap()],
            },
        ),
        ("main".to_string(), Registry::Router { sources: vec!["acme".to_string()] }),
        ("zstd".to_string(), Registry::Router { sources: vec!["acme".to_string()] }),
    ];
    config.routing.registries =
        Registries::new(graph.into_iter().collect(), Some("main".to_string()));
    config.routing.tar_registries.insert("zstd".to_string());
    config
}

async fn get(
    app: &axum::Router,
    uri: &str,
    accept_encoding: Option<&str>,
) -> axum::response::Response {
    let mut request = Request::get(uri);
    if let Some(encoding) = accept_encoding {
        request = request.header(header::ACCEPT_ENCODING, encoding);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn a_tar_registry_describes_and_serves_the_tar_of_a_hosted_tgz() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(acme_config(tmp.path().to_path_buf()), auth);
    let tar = widget_tar();
    let tgz = gzip(&tar);

    let publish = app
        .clone()
        .oneshot(hosted_publish_request(
            "/~acme/@acme/widget",
            "@acme/widget",
            "1.0.0",
            &tgz,
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    let packument = body_json(get(&app, "/~zstd/@acme%2Fwidget", None).await.into_body()).await;
    let dist = &packument["versions"]["1.0.0"]["dist"];
    dbg!(dist);
    assert_eq!(dist["integrity"], sha512_integrity(&tar));
    for omitted in ["tarball", "shasum", "signatures"] {
        assert!(dist.get(omitted).is_none(), "a tar registry omits dist.{omitted}");
    }

    let response = get(&app, "/~zstd/@acme/widget@1.0.0", Some("zstd")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/x-tar");
    assert_eq!(response.headers()[header::CONTENT_ENCODING], "zstd");
    let decoded = zstd::decode_all(body_bytes(response.into_body()).await.as_slice()).unwrap();
    assert_eq!(decoded, tar);

    let identity = get(&app, "/~zstd/@acme/widget@1.0.0", None).await;
    let identity_encoding = identity.headers().get(header::CONTENT_ENCODING);
    assert!(identity_encoding.is_none(), "{identity_encoding:?}");
    assert_eq!(body_bytes(identity.into_body()).await.as_slice(), tar.as_slice());

    // The `.tgz` registry serves the published bytes at the same path.
    let tgz_response = get(&app, "/@acme/widget@1.0.0", Some("zstd")).await;
    assert_eq!(tgz_response.status(), StatusCode::OK);
    assert_eq!(body_bytes(tgz_response.into_body()).await.as_slice(), tgz.as_slice());
    let packument = body_json(get(&app, "/@acme%2Fwidget", None).await.into_body()).await;
    assert_eq!(packument["versions"]["1.0.0"]["dist"]["integrity"], sha512_integrity(&tgz));
}

#[tokio::test]
async fn a_tar_registry_serves_no_tgz_path() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(acme_config(tmp.path().to_path_buf()), auth);
    let tgz = gzip(&widget_tar());
    let publish = app
        .clone()
        .oneshot(hosted_publish_request(
            "/~acme/@acme/widget",
            "@acme/widget",
            "1.0.0",
            &tgz,
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    let tgz_path = get(&app, "/~zstd/@acme/widget/-/widget-1.0.0.tgz", None).await;
    assert_eq!(tgz_path.status(), StatusCode::NOT_FOUND);
    let main_path = get(&app, "/@acme/widget/-/widget-1.0.0.tgz", None).await;
    assert_eq!(main_path.status(), StatusCode::OK);
}

#[tokio::test]
async fn an_upstream_version_gains_its_tar_integrity_once_its_tar_was_served() {
    let tar = tar_of(&[("package/package.json", br#"{"name":"left-pad","version":"1.0.0"}"#)]);
    let tgz = gzip(&tar);
    let mut upstream = mockito::Server::new_async().await;
    let packument = serde_json::json!({
        "name": "left-pad",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": { "name": "left-pad", "version": "1.0.0", "dist": {
            "tarball": format!("{}/left-pad/-/left-pad-1.0.0.tgz", upstream.url()),
            "integrity": sha512_integrity(&tgz),
            "shasum": "0000000000000000000000000000000000000000",
        } } },
    });
    let _packument = upstream
        .mock("GET", "/left-pad")
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .create_async()
        .await;
    let _tarball = upstream
        .mock("GET", "/left-pad/-/left-pad-1.0.0.tgz")
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

    let before = body_json(get(&app, "/~zstd/left-pad", None).await.into_body()).await;
    assert!(
        before["versions"]["1.0.0"]["dist"].get("integrity").is_none(),
        "the tar integrity is unknown before pnpr holds the tgz: {before}",
    );

    let served = get(&app, "/~zstd/left-pad@1.0.0", None).await;
    assert_eq!(served.status(), StatusCode::OK);
    assert_eq!(body_bytes(served.into_body()).await, tar);

    // The integrity is recorded once the decode finished, off the response path.
    let mut after = serde_json::Value::Null;
    for _ in 0..50 {
        after = body_json(get(&app, "/~zstd/left-pad", None).await.into_body()).await;
        if after["versions"]["1.0.0"]["dist"].get("integrity").is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(after["versions"]["1.0.0"]["dist"]["integrity"], sha512_integrity(&tar));
    assert!(
        tmp.path()
            .join(".tar-integrity/left-pad/package.json")
            .is_file(),
        "the tar integrity lives in the shared hosted storage",
    );
}

#[tokio::test]
async fn a_gzip_client_gets_the_stored_tgz() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(acme_config(tmp.path().to_path_buf()), auth);
    let tgz = gzip(&widget_tar());
    let publish = app
        .clone()
        .oneshot(hosted_publish_request(
            "/~acme/@acme/widget",
            "@acme/widget",
            "1.0.0",
            &tgz,
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    let response = get(&app, "/~zstd/@acme/widget@1.0.0", Some("gzip")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_ENCODING], "gzip");
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/x-tar");
    assert_eq!(body_bytes(response.into_body()).await, tgz);
}

#[tokio::test]
async fn a_level_19_variant_replaces_the_streamed_encoding() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(acme_config(tmp.path().to_path_buf()), auth);
    let tar = widget_tar();
    let publish = app
        .clone()
        .oneshot(hosted_publish_request(
            "/~acme/@acme/widget",
            "@acme/widget",
            "1.0.0",
            &gzip(&tar),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    let streamed = get(&app, "/~zstd/@acme/widget@1.0.0", Some("zstd")).await;
    assert!(
        streamed
            .headers()
            .get(header::CONTENT_LENGTH)
            .is_none(),
        "the first zstd response is compressed as it streams",
    );
    assert_eq!(zstd::decode_all(body_bytes(streamed.into_body()).await.as_slice()).unwrap(), tar);

    let mut stored = None;
    for _ in 0..100 {
        let response = get(&app, "/~zstd/@acme/widget@1.0.0", Some("zstd")).await;
        if response
            .headers()
            .get(header::CONTENT_LENGTH)
            .is_some()
        {
            stored = Some(response);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let stored = stored.expect("the worker stores a level-19 variant");
    assert_eq!(stored.headers()[header::CONTENT_ENCODING], "zstd");
    assert_eq!(zstd::decode_all(body_bytes(stored.into_body()).await.as_slice()).unwrap(), tar);
}

#[tokio::test]
async fn an_uncached_upstream_gets_no_tar_variants() {
    let tar = tar_of(&[("package/package.json", br#"{"name":"left-pad","version":"1.0.0"}"#)]);
    let tgz = gzip(&tar);
    let mut upstream = mockito::Server::new_async().await;
    let packument = serde_json::json!({
        "name": "left-pad",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": { "name": "left-pad", "version": "1.0.0", "dist": {
            "tarball": format!("{}/left-pad/-/left-pad-1.0.0.tgz", upstream.url()),
            "integrity": sha512_integrity(&tgz),
        } } },
    });
    let _packument = upstream
        .mock("GET", "/left-pad")
        .with_header("content-type", "application/json")
        .with_body(packument.to_string())
        .create_async()
        .await;
    let _tarball = upstream
        .mock("GET", "/left-pad/-/left-pad-1.0.0.tgz")
        .with_body(&tgz)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.routing.upstreams.get_mut("npmjs").unwrap().cache = false;
    let graph = vec![
        ("npmjs".to_string(), Registry::Upstream { patterns: vec![] }),
        ("zstd".to_string(), Registry::Router { sources: vec!["npmjs".to_string()] }),
    ];
    config.routing.registries = Registries::new(graph.into_iter().collect(), None);
    config.routing.tar_registries.insert("zstd".to_string());
    let cache_dir = config.storage.cache_dir.clone();
    let app = router(config);

    for _ in 0..2 {
        let served = get(&app, "/~zstd/left-pad@1.0.0", Some("zstd, gzip")).await;
        assert_eq!(served.status(), StatusCode::OK);
        assert_eq!(served.headers()[header::CONTENT_ENCODING], "zstd", "zstd stays available");
        assert_eq!(zstd::decode_all(body_bytes(served.into_body()).await.as_slice()).unwrap(), tar);
    }
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let persisted: Vec<_> = walkdir::WalkDir::new(&cache_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .to_string_lossy()
                .contains("tar-zstd19")
        })
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.path().to_path_buf())
        .collect();
    assert!(persisted.is_empty(), "a cache: false upstream persists no tar: {persisted:?}");
}

/// A cached `.tgz` is served without a re-check against the packument. When
/// the packument has since moved to other bytes, the tar response must not
/// complete with the stale ones, or a client with no tar hash to check would
/// install them.
#[tokio::test]
async fn a_stale_cached_tgz_fails_the_tar_response_and_is_refetched() {
    for accept in [None, Some("gzip"), Some("zstd")] {
        stale_cached_tgz_scenario(accept).await;
    }
}

async fn stale_cached_tgz_scenario(accept: Option<&str>) {
    let old_tar = tar_of(&[("package/package.json", br#"{"name":"left-pad","version":"1.0.0"}"#)]);
    let new_tar = tar_of(&[("package/index.js", b"module.exports = 2\n")]);
    let (old_tgz, new_tgz) = (gzip(&old_tar), gzip(&new_tar));
    let mut upstream = mockito::Server::new_async().await;
    let tarball_url = format!("{}/left-pad/-/left-pad-1.0.0.tgz", upstream.url());
    let packument = |tgz: &[u8]| {
        serde_json::json!({
            "name": "left-pad",
            "dist-tags": { "latest": "1.0.0" },
            "versions": { "1.0.0": { "name": "left-pad", "version": "1.0.0", "dist": {
                "tarball": tarball_url,
                "integrity": sha512_integrity(tgz),
            } } },
        })
        .to_string()
    };
    let old_packument = upstream
        .mock("GET", "/left-pad")
        .with_body(packument(&old_tgz))
        .create_async()
        .await;
    let old_tarball = upstream
        .mock("GET", "/left-pad/-/left-pad-1.0.0.tgz")
        .with_body(&old_tgz)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.http.packument_ttl = std::time::Duration::ZERO;
    let graph = vec![
        ("npmjs".to_string(), Registry::Upstream { patterns: vec![] }),
        ("zstd".to_string(), Registry::Router { sources: vec!["npmjs".to_string()] }),
    ];
    config.routing.registries = Registries::new(graph.into_iter().collect(), None);
    config.routing.tar_registries.insert("zstd".to_string());
    let app = router(config);

    let cached = get(&app, "/~zstd/left-pad@1.0.0", None).await;
    assert_eq!(body_bytes(cached.into_body()).await, old_tar, "the first fetch caches the old tgz");

    old_packument.remove_async().await;
    old_tarball.remove_async().await;
    let _new_packument = upstream
        .mock("GET", "/left-pad")
        .with_body(packument(&new_tgz))
        .create_async()
        .await;
    let _new_tarball = upstream
        .mock("GET", "/left-pad/-/left-pad-1.0.0.tgz")
        .with_body(&new_tgz)
        .create_async()
        .await;

    assert!(
        !completes_over_http(&app, "/~zstd/left-pad@1.0.0", accept).await,
        "stale bytes must not complete a tar response ({accept:?})",
    );

    let mut refetched = Vec::new();
    for _ in 0..50 {
        let response = get(&app, "/~zstd/left-pad@1.0.0", None).await;
        if let Ok(body) = axum::body::to_bytes(response.into_body(), usize::MAX).await {
            refetched = body.to_vec();
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(refetched, new_tar, "the evicted tgz is refetched from the upstream ({accept:?})");
}

/// Whether a GET of `path` completes over a real connection, as a client sees
/// it: one that stops at a declared `Content-Length` never sees an error the
/// server reports after the last byte.
async fn completes_over_http(app: &axum::Router, path: &str, accept: Option<&str>) -> bool {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(axum::serve(listener, app.clone()).into_future());
    let client = reqwest::Client::builder()
        .no_gzip()
        .no_zstd()
        .build()
        .unwrap();
    let mut request = client.get(format!("http://{addr}{path}"));
    if let Some(accept) = accept {
        request = request.header(header::ACCEPT_ENCODING, accept);
    }
    let completed = match request.send().await {
        Ok(response) => response.status().is_success() && response.bytes().await.is_ok(),
        Err(_) => false,
    };
    server.abort();
    completed
}

#[tokio::test]
async fn a_tar_registry_serves_no_integrity_addressed_tgz() {
    let tmp = TempDir::new().unwrap();
    let app = router(acme_config(tmp.path().to_path_buf()));
    let digest = "a".repeat(86);
    let response = get(&app, &format!("/~zstd/-/tarballs/sha512/{digest}"), None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn publishing_without_a_tar_registry_derives_no_tar_hash() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let mut config = acme_config(tmp.path().to_path_buf());
    config.routing.tar_registries.clear();
    let app = router_with_auth(config, auth);
    let publish = app
        .clone()
        .oneshot(hosted_publish_request(
            "/~acme/@acme/widget",
            "@acme/widget",
            "1.0.0",
            &gzip(&widget_tar()),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);
    assert!(
        !tmp.path()
            .join(".tar-integrity")
            .exists(),
        "no tar registry serves a tar hash",
    );
}

/// A version that only carries the legacy SHA-1 `shasum` still gets a tar.
#[tokio::test]
async fn a_shasum_only_upstream_version_is_served_as_a_tar() {
    let tar = tar_of(&[("package/package.json", br#"{"name":"old-pkg","version":"1.0.0"}"#)]);
    let tgz = gzip(&tar);
    let mut upstream = mockito::Server::new_async().await;
    let packument = serde_json::json!({
        "name": "old-pkg",
        "versions": { "1.0.0": { "name": "old-pkg", "version": "1.0.0", "dist": {
            "tarball": format!("{}/old-pkg/-/old-pkg-1.0.0.tgz", upstream.url()),
            "shasum": super::sha1_hex_of(&tgz),
        } } },
    });
    let _packument = upstream
        .mock("GET", "/old-pkg")
        .with_body(packument.to_string())
        .create_async()
        .await;
    let _tarball = upstream
        .mock("GET", "/old-pkg/-/old-pkg-1.0.0.tgz")
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

    let served = get(&app, "/~zstd/old-pkg@1.0.0", None).await;
    assert_eq!(served.status(), StatusCode::OK);
    assert_eq!(body_bytes(served.into_body()).await, tar);
}
