use super::{
    AuthState, Body, Config, Ecosystem, PackagePattern, Registries, Registry, Request, ServiceExt,
    StatusCode, TempDir, body_bytes, body_json, config_for, enable_osv, gzip, header,
    hosted_publish_request, hosted_with_access, integrity_addressed_tarball_path, osv_database,
    router, router_with_auth, sha512_integrity, tar_of,
};
use crate::tokens::issue_token;
use pnpr_storage::layout::encode_segment;

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
            "/npm/~acme/@acme/widget",
            "@acme/widget",
            "1.0.0",
            &tgz,
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    let packument = body_json(get(&app, "/npm/~zstd/@acme%2Fwidget", None).await.into_body()).await;
    let dist = &packument["versions"]["1.0.0"]["dist"];
    assert_eq!(dist["integrity"], sha512_integrity(&tar));
    assert_eq!(dist["fileCount"], serde_json::json!(1));
    assert_eq!(dist["unpackedSize"], serde_json::json!(41));
    assert_eq!(dist["pnpr"]["requiresBuild"], serde_json::json!(false));
    for omitted in ["tarball", "shasum", "signatures"] {
        assert!(dist.get(omitted).is_none(), "a tar registry omits dist.{omitted}");
    }

    let response = get(&app, "/npm/~zstd/@acme/widget@1.0.0", Some("zstd")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/x-tar");
    assert_eq!(response.headers()[header::CONTENT_ENCODING], "zstd");
    let decoded = zstd::decode_all(body_bytes(response.into_body()).await.as_slice()).unwrap();
    assert_eq!(decoded, tar);

    let identity = get(&app, "/npm/~zstd/@acme/widget@1.0.0", None).await;
    let identity_encoding = identity.headers().get(header::CONTENT_ENCODING);
    assert!(identity_encoding.is_none(), "{identity_encoding:?}");
    assert_eq!(body_bytes(identity.into_body()).await.as_slice(), tar.as_slice());

    // The `.tgz` registry serves the published bytes at the same path.
    let tgz_response = get(&app, "/npm/@acme/widget@1.0.0", Some("zstd")).await;
    assert_eq!(tgz_response.status(), StatusCode::OK);
    assert_eq!(body_bytes(tgz_response.into_body()).await.as_slice(), tgz.as_slice());
    let packument = body_json(get(&app, "/npm/@acme%2Fwidget", None).await.into_body()).await;
    assert_eq!(packument["versions"]["1.0.0"]["dist"]["integrity"], sha512_integrity(&tgz));
    assert_eq!(packument["versions"]["1.0.0"]["dist"]["fileCount"], serde_json::json!(1));
    assert_eq!(packument["versions"]["1.0.0"]["dist"]["unpackedSize"], serde_json::json!(41));
    assert_eq!(
        packument["versions"]["1.0.0"]["dist"]["pnpr"]["requiresBuild"],
        serde_json::json!(false),
    );
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
            "/npm/~acme/@acme/widget",
            "@acme/widget",
            "1.0.0",
            &tgz,
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    let tgz_path = get(&app, "/npm/~zstd/@acme/widget/-/widget-1.0.0.tgz", None).await;
    assert_eq!(tgz_path.status(), StatusCode::NOT_FOUND);
    let main_path = get(&app, "/npm/@acme/widget/-/widget-1.0.0.tgz", None).await;
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

    let before = body_json(get(&app, "/npm/~zstd/left-pad", None).await.into_body()).await;
    assert!(
        before["versions"]["1.0.0"]["dist"].get("integrity").is_none(),
        "the tar integrity is unknown before pnpr holds the tgz: {before}",
    );

    let served = get(&app, "/npm/~zstd/left-pad@1.0.0", None).await;
    assert_eq!(served.status(), StatusCode::OK);
    assert_eq!(body_bytes(served.into_body()).await, tar);

    // The integrity is recorded once the decode finished, off the response path.
    let mut after = serde_json::Value::Null;
    for _ in 0..50 {
        after = body_json(get(&app, "/npm/~zstd/left-pad", None).await.into_body()).await;
        if after["versions"]["1.0.0"]["dist"].get("integrity").is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(after["versions"]["1.0.0"]["dist"]["integrity"], sha512_integrity(&tar));
    assert!(
        tmp.path()
            .join(".tar-integrity/packages")
            .join(encode_segment("left-pad"))
            .join("document.json")
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
            "/npm/~acme/@acme/widget",
            "@acme/widget",
            "1.0.0",
            &tgz,
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    let response = get(&app, "/npm/~zstd/@acme/widget@1.0.0", Some("gzip")).await;
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
            "/npm/~acme/@acme/widget",
            "@acme/widget",
            "1.0.0",
            &gzip(&tar),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    let streamed = get(&app, "/npm/~zstd/@acme/widget@1.0.0", Some("zstd")).await;
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
        let response = get(&app, "/npm/~zstd/@acme/widget@1.0.0", Some("zstd")).await;
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
        let served = get(&app, "/npm/~zstd/left-pad@1.0.0", Some("zstd, gzip")).await;
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

    let cached = get(&app, "/npm/~zstd/left-pad@1.0.0", None).await;
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
        !completes_over_http(&app, "/npm/~zstd/left-pad@1.0.0", accept).await,
        "stale bytes must not complete a tar response ({accept:?})",
    );

    let mut refetched = Vec::new();
    for _ in 0..50 {
        let response = get(&app, "/npm/~zstd/left-pad@1.0.0", None).await;
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
async fn every_registry_serves_a_published_tar_by_its_digest() {
    let tmp = TempDir::new().unwrap();
    let mut config = acme_config(tmp.path().to_path_buf());
    // The tar is derived at publish even with no tar registry configured.
    config.routing.tar_registries.clear();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let tar = widget_tar();
    let tgz = gzip(&tar);
    let publish = router_with_auth(config.clone(), auth.clone())
        .oneshot(hosted_publish_request(
            "/npm/~acme/@acme/widget",
            "@acme/widget",
            "1.0.0",
            &tgz,
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);
    assert!(
        tmp.path()
            .join(".tar-integrity")
            .exists(),
        "publish records the tar binding",
    );

    config.routing.tar_registries.insert("zstd".to_string());
    let app = router_with_auth(config, auth);
    let tar_integrity = sha512_integrity(&tar).parse().unwrap();
    let revision_path = integrity_addressed_tarball_path(&tar_integrity).unwrap();
    let tgz_integrity = sha512_integrity(&tgz).parse().unwrap();
    let tgz_path = integrity_addressed_tarball_path(&tgz_integrity).unwrap();
    for registry in ["main", "zstd"] {
        let response = get(&app, &format!("/npm/~{registry}/{revision_path}"), None).await;
        assert_eq!(response.status(), StatusCode::OK, "{registry}");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/x-tar");
        assert_eq!(body_bytes(response.into_body()).await, tar, "{registry}");
        let by_tgz = get(&app, &format!("/npm/~{registry}/{tgz_path}"), None).await;
        assert_eq!(by_tgz.status(), StatusCode::NOT_FOUND, "the .tgz digest names no revision");
    }
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

    let served = get(&app, "/npm/~zstd/old-pkg@1.0.0", None).await;
    assert_eq!(served.status(), StatusCode::OK);
    assert_eq!(body_bytes(served.into_body()).await, tar);
}

#[tokio::test]
async fn every_document_and_route_form_of_a_tar_registry_describes_the_tar() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(acme_config(tmp.path().to_path_buf()), auth);
    let tar = widget_tar();
    let publish = app
        .clone()
        .oneshot(hosted_publish_request(
            "/npm/~acme/@acme/widget",
            "@acme/widget",
            "1.0.0",
            &gzip(&tar),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    let version =
        body_json(get(&app, "/npm/~zstd/@acme%2Fwidget/1.0.0", None).await.into_body()).await;
    dbg!(&version["dist"]);
    assert_eq!(version["dist"]["integrity"], sha512_integrity(&tar));
    assert_eq!(version["dist"]["pnpr"]["requiresBuild"], serde_json::json!(false));
    assert!(version["dist"].get("tarball").is_none());

    let tgz_version =
        body_json(get(&app, "/npm/@acme%2Fwidget/1.0.0", None).await.into_body()).await;
    assert_eq!(tgz_version["dist"]["pnpr"]["requiresBuild"], serde_json::json!(false));

    let abbreviated = app
        .clone()
        .oneshot(
            Request::get("/npm/~zstd/@acme%2Fwidget")
                .header(header::ACCEPT, "application/vnd.npm.install-v1+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let abbreviated = body_json(abbreviated.into_body()).await;
    let dist = &abbreviated["versions"]["1.0.0"]["dist"];
    dbg!(dist);
    assert_eq!(dist["integrity"], sha512_integrity(&tar));
    assert_eq!(dist["pnpr"]["requiresBuild"], serde_json::json!(false));
    assert!(dist.get("tarball").is_none());

    let tgz_abbreviated = app
        .clone()
        .oneshot(
            Request::get("/npm/@acme%2Fwidget")
                .header(header::ACCEPT, "application/vnd.npm.install-v1+json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let tgz_abbreviated = body_json(tgz_abbreviated.into_body()).await;
    assert_eq!(
        tgz_abbreviated["versions"]["1.0.0"]["dist"]["pnpr"]["requiresBuild"],
        serde_json::json!(false),
    );

    let encoded = get(&app, "/npm/~zstd/@acme%2Fwidget@1.0.0", None).await;
    assert_eq!(encoded.status(), StatusCode::OK);
    assert_eq!(body_bytes(encoded.into_body()).await.as_slice(), tar.as_slice());
}

#[tokio::test]
async fn the_version_route_refuses_a_vulnerable_version() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let osv = osv_database("@acme/widget", &["1.0.0"]);
    let mut config = acme_config(tmp.path().to_path_buf());
    enable_osv(&mut config, osv.path());
    let app = router_with_auth(config, auth);
    let publish = app
        .clone()
        .oneshot(hosted_publish_request(
            "/npm/~acme/@acme/widget",
            "@acme/widget",
            "1.0.0",
            &gzip(&widget_tar()),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    for path in ["/npm/~zstd/@acme/widget@1.0.0", "/npm/@acme/widget@1.0.0"] {
        let response = get(&app, path, Some("zstd")).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
    }
}

#[tokio::test]
async fn spoofed_dist_pnpr_is_stripped_and_replaced_by_tar_verified_requires_build() {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(acme_config(tmp.path().to_path_buf()), auth);
    // Tarball has a binding.gyp (so requiresBuild is true), while the publish
    // request attempts to spoof `dist.pnpr: {"requiresBuild": false, "spoof": 1}`.
    let tar = tar_of(&[
        ("package/package.json", br#"{"name":"@acme/native","version":"1.0.0"}"#),
        ("package/binding.gyp", b"{}"),
    ]);
    let tgz = gzip(&tar);
    let body = serde_json::json!({
        "name": "@acme/native",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "@acme/native",
                "version": "1.0.0",
                "dist": {
                    "tarball": "http://example.test/npm/@acme/native/-/native-1.0.0.tgz",
                    "integrity": sha512_integrity(&tgz),
                    "fileCount": 999,
                    "unpackedSize": 999_999,
                    "pnpr": { "requiresBuild": false, "spoof": 1 },
                },
            },
        },
        "_attachments": {
            "@acme/native-1.0.0.tgz": {
                "content_type": "application/octet-stream",
                "data": BASE64.encode(&tgz),
                "length": tgz.len(),
            },
        },
    });
    let publish = app
        .clone()
        .oneshot(
            Request::put("/npm/~acme/@acme/native")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    let packument = body_json(get(&app, "/npm/@acme%2Fnative", None).await.into_body()).await;
    let dist = &packument["versions"]["1.0.0"]["dist"];
    assert_eq!(dist["fileCount"], serde_json::json!(2));
    assert_eq!(dist["unpackedSize"], serde_json::json!(43));
    assert_eq!(dist["pnpr"], serde_json::json!({ "requiresBuild": true }));

    // Round-tripping the served packument (which carries `dist.pnpr`) through
    // the partial-unpublish `PUT /:pkg/-rev/:rev` endpoint must strip
    // `dist.pnpr` rather than rejecting it as an extra immutable `dist` field.
    let rewrite = app
        .clone()
        .oneshot(
            Request::put("/npm/~acme/@acme%2Fnative/-rev/1-0")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(packument.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rewrite.status(), StatusCode::CREATED);

    let mut omitted = packument.clone();
    let omitted_dist = omitted["versions"]["1.0.0"]["dist"].as_object_mut().unwrap();
    omitted_dist.remove("fileCount");
    omitted_dist.remove("unpackedSize");
    let rewrite_omitted = app
        .clone()
        .oneshot(
            Request::put("/npm/~acme/@acme%2Fnative/-rev/2-0")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(omitted.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rewrite_omitted.status(), StatusCode::CREATED);

    let after = body_json(get(&app, "/npm/@acme%2Fnative", None).await.into_body()).await;
    let after_dist = &after["versions"]["1.0.0"]["dist"];
    assert_eq!(after_dist["fileCount"], serde_json::json!(2));
    assert_eq!(after_dist["unpackedSize"], serde_json::json!(43));
    assert_eq!(after_dist["pnpr"], serde_json::json!({ "requiresBuild": true }));

    // Simulate a legacy hosted packument on disk that was published before
    // `fileCount`/`unpackedSize` were persisted in `package.json`, while
    // `.tar-integrities` has the verified values: round-tripping `GET /:pkg`
    // into `PUT /:pkg/-rev/:rev` must still succeed.
    let legacy_path = pnpr_storage::layout::local_document_path(tmp.path(), "acme", "@acme/native");
    let mut legacy_doc: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&legacy_path).unwrap()).unwrap();
    let legacy_dist = legacy_doc["versions"]["1.0.0"]["dist"].as_object_mut().unwrap();
    legacy_dist.remove("fileCount");
    legacy_dist.remove("unpackedSize");
    std::fs::write(&legacy_path, serde_json::to_vec_pretty(&legacy_doc).unwrap()).unwrap();

    let served_legacy = body_json(get(&app, "/npm/@acme%2Fnative", None).await.into_body()).await;
    let rewrite_legacy = app
        .clone()
        .oneshot(
            Request::put("/npm/~acme/@acme%2Fnative/-rev/3-0")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(served_legacy.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rewrite_legacy.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn published_tar_surfaces_precalculated_bins_and_shebang_runtimes_in_dist_pnpr() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(acme_config(tmp.path().to_path_buf()), auth);
    let tar = tar_of(&[
        (
            "package/package.json",
            br#"{"name":"@acme/cli","version":"1.0.0","bin":{"acme-cli":"./bin/cli.js","acme-sh":"./bin/run"}}"#,
        ),
        (
            "package/bin/cli.js",
            b"#!/usr/bin/env -S node --no-warnings\nconsole.log('hi');\n",
        ),
        ("package/bin/run", b"#!/bin/sh\necho hi\n"),
    ]);
    let publish = app
        .clone()
        .oneshot(hosted_publish_request(
            "/npm/~acme/@acme/cli",
            "@acme/cli",
            "1.0.0",
            &gzip(&tar),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    let expected_pnpr = serde_json::json!({
        "requiresBuild": false,
        "bins": {
            "acme-cli": {
                "path": "bin/cli.js",
                "prog": "node",
                "args": " --no-warnings",
            },
            "acme-sh": {
                "path": "bin/run",
                "prog": "/bin/sh",
            },
        },
    });

    let packument = body_json(get(&app, "/npm/@acme%2Fcli", None).await.into_body()).await;
    assert_eq!(packument["versions"]["1.0.0"]["dist"]["pnpr"], expected_pnpr);

    let version =
        body_json(get(&app, "/npm/~zstd/@acme%2Fcli/1.0.0", None).await.into_body()).await;
    assert_eq!(version["dist"]["pnpr"], expected_pnpr);
}

#[tokio::test]
async fn pathological_bin_counts_omit_dist_pnpr_bins_and_preserve_requires_build() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(acme_config(tmp.path().to_path_buf()), auth);

    let mut entries: Vec<(String, Vec<u8>)> = vec![(
        "package/package.json".to_string(),
        br#"{"name":"@acme/many-bins","version":"1.0.0","directories":{"bin":"./bin"}}"#.to_vec(),
    )];
    for index in 0..=pnpm_package_manifest::MAX_PRECALCULATED_BINS {
        entries.push((format!("package/bin/cmd-{index}.js"), b"#!/usr/bin/env node\n".to_vec()));
    }
    let borrowed: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect();
    let tar = tar_of(&borrowed);

    let publish = app
        .clone()
        .oneshot(hosted_publish_request(
            "/npm/~acme/@acme/many-bins",
            "@acme/many-bins",
            "1.0.0",
            &gzip(&tar),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    let packument = body_json(get(&app, "/npm/@acme%2Fmany-bins", None).await.into_body()).await;
    assert_eq!(
        packument["versions"]["1.0.0"]["dist"]["pnpr"],
        serde_json::json!({ "requiresBuild": false }),
    );
}

async fn put_json(
    app: &axum::Router,
    uri: &str,
    token: &str,
    body: &serde_json::Value,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::put(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(serde_json::to_vec(body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn a_deprecate_through_a_tar_registry_keeps_the_stored_dist() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(acme_config(tmp.path().to_path_buf()), auth);
    let tgz = gzip(&widget_tar());
    let publish = app
        .clone()
        .oneshot(hosted_publish_request(
            "/npm/~acme/@acme/widget",
            "@acme/widget",
            "1.0.0",
            &tgz,
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);
    let stored = body_json(get(&app, "/npm/@acme%2Fwidget", None).await.into_body()).await;

    let mut packument =
        body_json(get(&app, "/npm/~zstd/@acme%2Fwidget", None).await.into_body()).await;
    packument["versions"]["1.0.0"]["deprecated"] = serde_json::json!("use 2.0.0 instead");
    let response = put_json(&app, "/npm/~zstd/@acme%2Fwidget", &token, &packument).await;
    assert_eq!(response.status(), StatusCode::CREATED);

    let after = body_json(get(&app, "/npm/@acme%2Fwidget", None).await.into_body()).await;
    assert_eq!(after["versions"]["1.0.0"]["deprecated"], "use 2.0.0 instead");
    assert_eq!(after["versions"]["1.0.0"]["dist"], stored["versions"]["1.0.0"]["dist"]);
}

#[tokio::test]
async fn a_partial_unpublish_through_a_tar_registry_keeps_the_stored_dist_and_deletes_the_tarball()
{
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(acme_config(tmp.path().to_path_buf()), auth);
    for (version, content) in [("1.0.0", "a"), ("1.0.1", "b")] {
        let tar = tar_of(&[("package/index.js", content.as_bytes())]);
        let publish = app
            .clone()
            .oneshot(hosted_publish_request(
                "/npm/~acme/@acme/widget",
                "@acme/widget",
                version,
                &gzip(&tar),
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(publish.status(), StatusCode::CREATED);
    }
    let stored = body_json(get(&app, "/npm/@acme%2Fwidget", None).await.into_body()).await;

    let mut packument =
        body_json(get(&app, "/npm/~zstd/@acme%2Fwidget", None).await.into_body()).await;
    assert_ne!(
        packument["versions"]["1.0.0"]["dist"]["integrity"],
        stored["versions"]["1.0.0"]["dist"]["integrity"],
    );
    packument["versions"]
        .as_object_mut()
        .unwrap()
        .remove("1.0.1");
    packument["dist-tags"]["latest"] = serde_json::json!("1.0.0");
    let response = put_json(&app, "/npm/~zstd/@acme%2Fwidget/-rev/1-0", &token, &packument).await;
    assert_eq!(response.status(), StatusCode::CREATED);

    let after = body_json(get(&app, "/npm/@acme%2Fwidget", None).await.into_body()).await;
    assert!(after["versions"].get("1.0.1").is_none());
    assert_eq!(after["versions"]["1.0.0"]["dist"], stored["versions"]["1.0.0"]["dist"]);

    let kept = get(&app, "/npm/@acme/widget/-/widget-1.0.0.tgz", None).await;
    assert_eq!(kept.status(), StatusCode::OK);
    let removed = get(&app, "/npm/@acme/widget/-/widget-1.0.1.tgz", None).await;
    assert_eq!(removed.status(), StatusCode::NOT_FOUND);

    let client_delete = app
        .clone()
        .oneshot(
            Request::delete("/npm/@acme/widget/-/widget-1.0.1.tgz/-rev/2-0")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(client_delete.status(), StatusCode::CREATED);
}
