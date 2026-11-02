use super::{
    AuthState, Body, Config, Ecosystem, HeaderValue, PackagePattern, PackageRules, Registries,
    Registry, Request, Router, ServiceExt, StatusCode, TempDir, Value, access_rule, body_bytes,
    config_for, enable_osv, gzip, header, hosted_publish_request, hosted_with_access, json,
    osv_database, package_tar_and_tgz, router_config, router_with_auth, sha512_integrity, tar_of,
    upstream_endpoint_config,
};
use crate::tokens::issue_token;

/// An upstream-backed registry whose upstream sends `x-upstream-auth`, and
/// the bearer authorization of `alice`, who may read it.
async fn revision_upstream_app(upstream: &mockito::Server, storage: &TempDir) -> (Router, String) {
    let mut config =
        upstream_endpoint_config(&upstream.url(), storage.path().to_path_buf(), "alice");
    config.routing.upstreams
        .get_mut("npmjs")
        .unwrap()
        .headers
        .insert("x-upstream-auth", HeaderValue::from_static("secret"));
    config.routing.registries = Registries::new(
        std::iter::once(("npmjs".to_string(), Registry::Upstream { patterns: Vec::new() }))
            .collect(),
        Some("npmjs".to_string()),
    );
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    (router_with_auth(config, auth), format!("Bearer {token}"))
}

/// A version whose selected revision is `selected`, with an original and a
/// first replacement in its history.
fn revision_version(
    upstream_url: &str,
    original: &[u8],
    replacement: &[u8],
    selected: u64,
) -> Value {
    let record = |revision: u64, tar: &[u8]| {
        let integrity_text = sha512_integrity(tar);
        json!({
            "revision": revision,
            "integrity": integrity_text,
            "tarball": format!("{upstream_url}/foo@1.0.0+r{revision}"),
            "manifest": {},
        })
    };
    let history = vec![record(0, original), record(1, replacement)];
    let current = &history[usize::try_from(selected).unwrap()];
    json!({ "name": "foo", "version": "1.0.0", "dist": {
        "tarball": current["tarball"],
        "integrity": current["integrity"],
        "shasum": "0000000000000000000000000000000000000000",
        "signatures": [{ "keyid": "SHA256:key", "sig": "signature" }],
        "revision": selected,
        "revisions": history,
    } })
}

/// The response for `path` in pnpr's own format.
async fn get_pnpr_format(app: &Router, path: &str, authorization: &str) -> Value {
    let request = Request::get(path)
        .header(header::AUTHORIZATION, authorization)
        .header(header::ACCEPT, "application/vnd.pnpr.install-v2+json")
        .body(Body::empty())
        .unwrap();
    let response = app
        .clone()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap()
}

async fn get_with(
    app: &Router,
    path: &str,
    authorization: &str,
    accept_encoding: Option<&str>,
) -> axum::response::Response {
    let mut request = Request::get(path).header(header::AUTHORIZATION, authorization);
    if let Some(coding) = accept_encoding {
        request = request.header(header::ACCEPT_ENCODING, coding);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn upstream_endpoint_preserves_and_serves_revision_tarballs() {
    let mut upstream = mockito::Server::new_async().await;
    let (original, _) = package_tar_and_tgz(r#"{"name":"foo","version":"1.0.0"}"#);
    let (replacement, replacement_tgz) =
        package_tar_and_tgz(r#"{"name":"foo","version":"1.0.0","patched":true}"#);
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": revision_version(&upstream.url(), &original, &replacement, 1) },
    });
    let packument_mock = upstream
        .mock("GET", "/foo")
        .match_header("x-upstream-auth", "secret")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", "/foo@1.0.0+r1")
        .match_header("x-upstream-auth", "secret")
        .match_header("accept-encoding", "gzip")
        .with_header("content-encoding", "gzip")
        .with_body(&replacement_tgz)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let (app, authorization) = revision_upstream_app(&upstream, &tmp).await;

    let response = get_with(&app, "/npm/~npmjs/foo", &authorization, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    let dist = &body["versions"]["1.0.0"]["dist"];
    // npm's document gives the selected revision as npm clients read it: its
    // tarball and integrity, without the revision protocol's fields.
    assert_eq!(dist["tarball"], "http://example.test/npm/~npmjs/foo@1.0.0+r1");
    assert_eq!(dist["integrity"], sha512_integrity(&replacement));
    for omitted in ["shasum", "signatures", "revision", "revisions"] {
        assert!(dist.get(omitted).is_none(), "npm's document omits dist.{omitted}");
    }
    let document = get_pnpr_format(&app, "/npm/~npmjs/foo", &authorization).await;
    let entry = &document["versions"]["1.0.0"];
    assert_eq!(entry["revision"], 1);
    assert_eq!(entry["integrity"], sha512_integrity(&replacement));
    assert!(entry.get("tarball").is_none() && entry.get("revisions").is_none());
    // Each revision in the history has its release record.
    let original_record = get_pnpr_format(&app, "/npm/~npmjs/foo/1.0.0+r0", &authorization).await;
    assert_eq!(original_record["integrity"], sha512_integrity(&original));
    assert_eq!(original_record["version"], "1.0.0");
    let selected_record = get_pnpr_format(&app, "/npm/~npmjs/foo/1.0.0+r1", &authorization).await;
    assert_eq!(selected_record["integrity"], sha512_integrity(&replacement));

    for path in ["/npm/~npmjs/foo@1.0.0+r1", "/npm/foo@1.0.0+r1"] {
        let response = get_with(&app, path, &authorization, None).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/x-tar");
        assert!(
            response
                .headers()
                .get(header::ETAG)
                .is_none(),
        );
        assert_eq!(body_bytes(response.into_body()).await, replacement);

        let gzip = get_with(&app, path, &authorization, Some("gzip")).await;
        assert_eq!(gzip.headers()[header::CONTENT_ENCODING], "gzip");
        assert_eq!(body_bytes(gzip.into_body()).await, replacement_tgz);
    }

    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn upstream_revision_zero_is_projected_explicitly() {
    let mut upstream = mockito::Server::new_async().await;
    let (original, _) = package_tar_and_tgz(r#"{"name":"foo","version":"1.0.0"}"#);
    let (replacement, _) = package_tar_and_tgz(r#"{"name":"foo","version":"1.0.0","patched":1}"#);
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": revision_version(&upstream.url(), &original, &replacement, 0) },
    });
    let _packument = upstream
        .mock("GET", "/foo")
        .with_body(packument.to_string())
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let (app, authorization) = revision_upstream_app(&upstream, &tmp).await;

    let response = get_with(&app, "/npm/~npmjs/foo", &authorization, None).await;
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    let dist = &body["versions"]["1.0.0"]["dist"];
    assert_eq!(dist["integrity"], sha512_integrity(&original));
    assert_eq!(dist["tarball"], "http://example.test/npm/~npmjs/foo@1.0.0+r0");
    assert!(dist.get("revision").is_none() && dist.get("revisions").is_none());
    // pnpr's format marks the original explicitly, so a client builds the
    // `+r0` URL its tar integrity belongs to.
    let document = get_pnpr_format(&app, "/npm/~npmjs/foo", &authorization).await;
    assert_eq!(document["versions"]["1.0.0"]["revision"], 0);
    let replacement_record =
        get_pnpr_format(&app, "/npm/~npmjs/foo/1.0.0+r1", &authorization).await;
    assert_eq!(replacement_record["integrity"], sha512_integrity(&replacement));
}

#[tokio::test]
async fn incomplete_upstream_revision_metadata_is_ordinary_metadata() {
    let mut upstream = mockito::Server::new_async().await;
    let (original, _) = package_tar_and_tgz(r#"{"name":"foo","version":"1.0.0"}"#);
    let (replacement, _) = package_tar_and_tgz(r#"{"name":"foo","version":"1.0.0","patched":1}"#);
    let mut without_original = revision_version(&upstream.url(), &original, &replacement, 1);
    without_original["dist"]["revisions"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    let mut without_marker = revision_version(&upstream.url(), &original, &replacement, 1);
    without_marker["dist"]
        .as_object_mut()
        .unwrap()
        .remove("revision");
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": without_original, "2.0.0": without_marker },
    });
    let _packument = upstream
        .mock("GET", "/foo")
        .with_body(packument.to_string())
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let (app, authorization) = revision_upstream_app(&upstream, &tmp).await;

    let response = get_with(&app, "/npm/~npmjs/foo", &authorization, None).await;
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    for version in ["1.0.0", "2.0.0"] {
        let dist = &body["versions"][version]["dist"];
        assert!(dist.get("revision").is_none(), "{version}");
        assert!(dist.get("revisions").is_none(), "{version}");
    }
}

#[tokio::test]
async fn an_identity_upstream_revision_is_served_in_every_coding() {
    let mut upstream = mockito::Server::new_async().await;
    let (tar, _) = package_tar_and_tgz(r#"{"name":"foo","version":"1.0.0"}"#);
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": revision_version(&upstream.url(), &tar, &tar, 0) },
    });
    let _packument_mock = upstream
        .mock("GET", "/foo")
        .with_body(packument.to_string())
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", "/foo@1.0.0+r0")
        .with_body(&tar)
        .expect(1)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let (app, authorization) = revision_upstream_app(&upstream, &tmp).await;

    let identity = get_with(&app, "/npm/~npmjs/foo@1.0.0+r0", &authorization, None).await;
    assert_eq!(identity.status(), StatusCode::OK);
    assert_eq!(identity.headers()[header::CONTENT_TYPE], "application/x-tar");
    assert!(
        identity.headers()[header::VARY]
            .to_str()
            .unwrap()
            .contains("accept-encoding"),
    );
    assert_eq!(body_bytes(identity.into_body()).await, tar);
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn a_revision_that_does_not_match_its_digest_is_neither_served_nor_cached() {
    let mut upstream = mockito::Server::new_async().await;
    let (tar, _) = package_tar_and_tgz(r#"{"name":"foo","version":"1.0.0"}"#);
    let (_, tampered_tgz) = package_tar_and_tgz(r#"{"name":"foo","version":"6.6.6"}"#);
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": revision_version(&upstream.url(), &tar, &tar, 0) },
    });
    let _packument_mock = upstream
        .mock("GET", "/foo")
        .with_body(packument.to_string())
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", "/foo@1.0.0+r0")
        .with_header("content-encoding", "gzip")
        .with_body(&tampered_tgz)
        .expect(2)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let (app, authorization) = revision_upstream_app(&upstream, &tmp).await;

    for _ in 0..2 {
        let response = get_with(&app, "/npm/~npmjs/foo@1.0.0+r0", &authorization, None).await;
        assert!(!response.status().is_success(), "{:?}", response.status());
    }
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn upstream_revision_tarball_does_not_follow_redirects() {
    let mut upstream = mockito::Server::new_async().await;
    let (tar, _) = package_tar_and_tgz(r#"{"name":"foo","version":"1.0.0"}"#);
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": revision_version(&upstream.url(), &tar, &tar, 0) },
    });
    let _packument_mock = upstream
        .mock("GET", "/foo")
        .with_body(packument.to_string())
        .create_async()
        .await;
    let redirect = upstream
        .mock("GET", "/foo@1.0.0+r0")
        .with_status(302)
        .with_header("location", "/redirected.tgz")
        .expect(1)
        .create_async()
        .await;
    let redirected = upstream
        .mock("GET", "/redirected.tgz")
        .expect(0)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let config = upstream_endpoint_config(&upstream.url(), tmp.path().to_path_buf(), "alice");
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let app = router_with_auth(config, auth);
    let response = app
        .oneshot(
            Request::get("/npm/~npmjs/foo@1.0.0+r0")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    redirect.assert_async().await;
    redirected.assert_async().await;
}

#[tokio::test]
async fn revision_tarballs_route_through_routers_and_enforce_package_access_rules() {
    let mut npmjs = mockito::Server::new_async().await;
    let mut corp = mockito::Server::new_async().await;
    let (tar, tgz) = package_tar_and_tgz(r#"{"name":"foo","version":"1.0.0"}"#);
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": revision_version(&npmjs.url(), &tar, &tar, 0) },
    });
    let _packument = npmjs
        .mock("GET", "/foo")
        .with_body(packument.to_string())
        .create_async()
        .await;
    let tarball = npmjs
        .mock("GET", "/foo@1.0.0+r0")
        .with_header("content-encoding", "gzip")
        .with_body(&tgz)
        .expect(1)
        .create_async()
        .await;
    let corp_tarball = corp
        .mock("GET", "/foo@1.0.0+r0")
        .expect(0)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let router_app = router_with_auth(
        router_config(&npmjs.url(), &corp.url(), tmp.path().join("router")),
        AuthState::in_memory(),
    );
    for path in ["/npm/~main/foo@1.0.0+r0", "/npm/foo@1.0.0+r0"] {
        let response = router_app
            .clone()
            .oneshot(
                Request::get(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(body_bytes(response.into_body()).await, tar);
    }

    let restricted_packument = npmjs
        .mock("GET", "/restricted")
        .expect(0)
        .create_async()
        .await;
    let restricted_tarball = npmjs
        .mock("GET", "/restricted@1.0.0+r0")
        .expect(0)
        .create_async()
        .await;
    let osv = osv_database("restricted", &["1.0.0"]);
    let mut config = config_for(&npmjs.url(), tmp.path().join("package-access"));
    enable_osv(&mut config, osv.path());
    config.routing.upstreams.get_mut("npmjs").unwrap().rules =
        PackageRules::new(vec![access_rule("restricted", "$authenticated")], None);
    let response = router_with_auth(config, AuthState::in_memory())
        .oneshot(
            Request::get("/npm/~npmjs/restricted@1.0.0+r0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    restricted_packument.assert_async().await;
    restricted_tarball.assert_async().await;
    tarball.assert_async().await;
    corp_tarball.assert_async().await;
}

#[tokio::test]
async fn hosted_original_is_served_at_revision_zero_after_restart_and_through_a_router() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.routing.hosted.insert("acme".to_string(), hosted_with_access("acme", "$all"));
    let graph = vec![
        (
            "acme".to_string(),
            Registry::Hosted {
                patterns: vec![PackagePattern::parse("@acme/*", Ecosystem::Npm).unwrap()],
            },
        ),
        ("main".to_string(), Registry::Router { sources: vec!["acme".to_string()] }),
    ];
    config.routing.registries =
        Registries::new(graph.into_iter().collect(), Some("main".to_string()));
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let (tar, tarball) = package_tar_and_tgz(r#"{"name":"@acme/widget","version":"1.0.0"}"#);

    let publish = router_with_auth(config.clone(), auth.clone())
        .oneshot(hosted_publish_request(
            "/npm/~acme/@acme/widget",
            "@acme/widget",
            "1.0.0",
            &tarball,
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    let app = router_with_auth(config, auth);
    for path in [
        "/npm/~acme/@acme/widget@1.0.0+r0",
        "/npm/~main/@acme/widget@1.0.0+r0",
        "/npm/@acme/widget@1.0.0+r0",
        "/npm/@acme%2Fwidget@1.0.0+r0",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::get(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let headers = response.headers();
        assert_eq!(headers[header::CACHE_CONTROL], "private, no-store");
        assert_eq!(headers[header::VARY], "accept-encoding, Authorization");
        assert_eq!(headers[header::CONTENT_TYPE], "application/x-tar");
        for absent in [header::ETAG.as_str(), "content-digest", header::CONTENT_ENCODING.as_str()] {
            assert!(headers.get(absent).is_none(), "{path} sends no {absent}");
        }
        assert_eq!(body_bytes(response.into_body()).await, tar, "{path}");
    }

    let gzip_response = app
        .clone()
        .oneshot(
            Request::get("/npm/@acme/widget@1.0.0+r0")
                .header(header::ACCEPT_ENCODING, "gzip")
                .header(header::RANGE, "bytes=0-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(gzip_response.status(), StatusCode::OK, "a range request gets the whole body");
    assert_eq!(gzip_response.headers()[header::CONTENT_ENCODING], "gzip");
    assert_eq!(body_bytes(gzip_response.into_body()).await, tarball);

    for missing in ["/npm/@acme/widget@1.0.0+r1", "/npm/@acme/widget@1.0.0+r00"] {
        let response = app
            .clone()
            .oneshot(
                Request::get(missing)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{missing}");
    }

    let canonical = app
        .oneshot(
            Request::get("/npm/~acme/@acme/widget/-/widget-1.0.0.tgz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(canonical.status(), StatusCode::OK);
    assert_eq!(body_bytes(canonical.into_body()).await, tarball);
}

#[tokio::test]
async fn hosted_revision_route_rechecks_package_access() {
    let tmp = TempDir::new().unwrap();
    let osv = osv_database("secret", &["2.0.0"]);
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    enable_osv(&mut config, osv.path());
    config.routing.hosted.insert("corp".to_string(), hosted_with_access("corp", "alice"));
    config.routing.registries = Registries::new(
        vec![("corp".to_string(), Registry::Hosted { patterns: vec![] })].into_iter().collect(),
        Some("corp".to_string()),
    );
    let auth = AuthState::in_memory();
    let alice = issue_token(&auth, "alice").await;
    let bob = issue_token(&auth, "bob").await;
    let (tar, tarball) = package_tar_and_tgz(r#"{"name":"secret","version":"1.0.0"}"#);
    let app = router_with_auth(config, auth);

    let publish = app
        .clone()
        .oneshot(hosted_publish_request("/npm/~corp/secret", "secret", "1.0.0", &tarball, &alice))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    for (path, authorization) in [
        ("/npm/~corp/secret@1.0.0+r0", None),
        ("/npm/~corp/secret@1.0.0+r0", Some(&bob)),
        ("/npm/~corp/secret@2.0.0+r0", None),
        ("/npm/~corp/secret@2.0.0+r0", Some(&bob)),
    ] {
        let mut request = Request::get(path);
        if let Some(token) = authorization {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(
            response
                .headers()
                .get(header::CACHE_CONTROL)
                .unwrap(),
            "private, no-store",
        );
        assert_eq!(
            response
                .headers()
                .get(header::VARY)
                .unwrap(),
            "Authorization",
        );
    }

    let response = app
        .oneshot(
            Request::get("/npm/~corp/secret@1.0.0+r0")
                .header(header::AUTHORIZATION, format!("Bearer {alice}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CACHE_CONTROL)
            .unwrap(),
        "private, no-store",
    );
    assert_eq!(
        response
            .headers()
            .get(header::VARY)
            .unwrap(),
        "accept-encoding, Authorization",
    );
    assert_eq!(body_bytes(response.into_body()).await, tar);
}

#[tokio::test]
async fn every_registry_serves_a_published_tar_at_revision_zero() {
    let tmp = TempDir::new().unwrap();
    let mut config: Config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
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
    let auth = AuthState::in_memory();
    let token = issue_token(&auth, "alice").await;
    let tar = tar_of(&[("package/package.json", br#"{"name":"@acme/widget","version":"1.0.0"}"#)]);
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
    for registry in ["main", "zstd"] {
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/npm/~{registry}/@acme/widget@1.0.0+r0"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{registry}");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/x-tar");
        assert_eq!(body_bytes(response.into_body()).await, tar, "{registry}");
        let replacement = app
            .clone()
            .oneshot(
                Request::get(format!("/npm/~{registry}/@acme/widget@1.0.0+r1"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(replacement.status(), StatusCode::NOT_FOUND, "only revision 0 exists");
    }
}
