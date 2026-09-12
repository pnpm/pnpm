use super::{
    AccessList, AuthState, Body, Ecosystem, PackagePattern, PackageRules, Registries, Registry,
    Request, ServiceExt, StatusCode, TempDir, Value, access_rule, body_json, config_for, header,
    hosted_with_access, json, router, router_with_auth, seed_hosted, sha512_integrity, to_bytes,
};

#[tokio::test]
async fn search_paginates_across_hosted_and_upstream_sources() {
    let mut upstream = mockito::Server::new_async().await;
    let shadowed = upstream
        .mock("GET", "/-/v1/search")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("text".into(), "ajv".into()),
            mockito::Matcher::UrlEncoded("from".into(), "0".into()),
            mockito::Matcher::UrlEncoded("size".into(), "250".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "objects": [
                    { "package": { "name": "ajv" } },
                    { "package": { "name": "ajv-keywords" } },
                ],
                "total": 4,
            })
            .to_string(),
        )
        .expect(2)
        .create_async()
        .await;
    let visible = upstream
        .mock("GET", "/-/v1/search")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("text".into(), "ajv".into()),
            mockito::Matcher::UrlEncoded("from".into(), "2".into()),
            mockito::Matcher::UrlEncoded("size".into(), "250".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "objects": [
                    { "package": { "name": "ajv-remote-a" } },
                    { "package": { "name": "ajv-remote-b" } },
                ],
                "total": 4,
            })
            .to_string(),
        )
        .expect(2)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    seed_hosted(tmp.path(), "ajv");
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.upstreams.get_mut("npmjs").unwrap().search = true;
    let app = router(config);

    let first_page = app
        .clone()
        .oneshot(Request::get("/-/v1/search?text=ajv&size=2").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(first_page.status(), StatusCode::OK);
    let first_page = body_json(first_page.into_body()).await;
    assert_eq!(first_page["total"], json!(3));
    assert_eq!(first_page["objects"][0]["package"]["name"], json!("ajv"));
    assert_eq!(first_page["objects"][1]["package"]["name"], json!("ajv-remote-a"));

    let second_page = app
        .oneshot(Request::get("/-/v1/search?text=ajv&from=2&size=1").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(second_page.status(), StatusCode::OK);
    let second_page = body_json(second_page.into_body()).await;
    assert_eq!(second_page["total"], json!(3));
    assert_eq!(second_page["objects"][0]["package"]["name"], json!("ajv-remote-b"));
    shadowed.assert_async().await;
    visible.assert_async().await;
}

/// Every registry operation routes through the registry graph when addressed as
/// `/~<name>/...` (RFC "registries", implementation point 7): dist-tag
/// read/add/remove, whoami, search, the version manifest, and the whole
/// unpublish flow. The registry here is deliberately *not* reachable from any
/// default target — without the registry-addressed surface these operations
/// would be impossible for it.
#[tokio::test]
async fn registry_addressed_surface_serves_dist_tags_unpublish_whoami_search_and_version_manifest()
{
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.hosted.insert("acme".to_string(), hosted_with_access("acme", "$authenticated"));
    // No default target: the registry is addressable only at `/~acme/`.
    config.registries = Registries::new(
        vec![("acme".to_string(), Registry::Hosted { patterns: vec![] })].into_iter().collect(),
        None,
    );
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config, auth);
    let authed = |request: axum::http::request::Builder| {
        request.header(header::AUTHORIZATION, format!("Bearer {token}"))
    };

    // Seed the registry through its own publish endpoint.
    let tarball = b"acme-widget-bytes";
    let publish_body = json!({
        "name": "@acme/widget",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": { "name": "@acme/widget", "version": "1.0.0", "dist": {
            "tarball": "http://example.test/@acme/widget/-/widget-1.0.0.tgz",
            "integrity": sha512_integrity(tarball),
        } } },
        "_attachments": { "@acme/widget-1.0.0.tgz": {
            "content_type": "application/octet-stream",
            "data": BASE64.encode(tarball),
            "length": tarball.len(),
        } },
    });
    let publish = app
        .clone()
        .oneshot(
            authed(Request::put("/~acme/@acme/widget"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&publish_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    // dist-tags read through the registry.
    let tags = app
        .clone()
        .oneshot(
            authed(Request::get("/~acme/-/package/@acme%2Fwidget/dist-tags"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(tags.status(), StatusCode::OK);
    assert_eq!(body_json(tags.into_body()).await["latest"], json!("1.0.0"));

    // dist-tag add, visible on the next read, then remove.
    let add = app
        .clone()
        .oneshot(
            authed(Request::put("/~acme/-/package/@acme%2Fwidget/dist-tags/beta"))
                .header("content-type", "application/json")
                .body(Body::from(r#""1.0.0""#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(add.status(), StatusCode::CREATED);
    let tags = app
        .clone()
        .oneshot(
            authed(Request::get("/~acme/-/package/@acme%2Fwidget/dist-tags"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(body_json(tags.into_body()).await["beta"], json!("1.0.0"));
    let remove = app
        .clone()
        .oneshot(
            authed(Request::delete("/~acme/-/package/@acme%2Fwidget/dist-tags/beta"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(remove.status(), StatusCode::CREATED);

    // whoami through the registry.
    let whoami = app
        .clone()
        .oneshot(authed(Request::get("/~acme/-/whoami")).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(whoami.status(), StatusCode::OK);
    assert_eq!(body_json(whoami.into_body()).await["username"], json!("alice"));

    // Search through the registry finds the hosted package; the anonymous
    // caller (whom the registry denies) gets an empty result.
    let search = app
        .clone()
        .oneshot(
            authed(Request::get("/~acme/-/v1/search?text=widget")).body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(search.status(), StatusCode::OK);
    let body = body_json(search.into_body()).await;
    assert_eq!(body["objects"][0]["package"]["name"], json!("@acme/widget"));
    let anon_search = app
        .clone()
        .oneshot(Request::get("/~acme/-/v1/search?text=widget").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(body_json(anon_search.into_body()).await["total"], json!(0));

    // Version manifest through the registry, with `dist.tarball` rewritten onto
    // the registry's own base.
    let manifest = app
        .clone()
        .oneshot(authed(Request::get("/~acme/@acme/widget/1.0.0")).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(manifest.status(), StatusCode::OK);
    let manifest = body_json(manifest.into_body()).await;
    assert_eq!(
        manifest["dist"]["tarball"],
        json!("http://example.test/~acme/@acme/widget/-/widget-1.0.0.tgz"),
    );

    // The unpublish flow: PUT back a packument without the version, delete
    // its tarball (the unencoded scoped 7-segment form), then delete the
    // package — all through the registry.
    let put_back = app
        .clone()
        .oneshot(
            authed(Request::put("/~acme/@acme%2Fwidget/-rev/1"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "name": "@acme/widget", "versions": {}, "dist-tags": {} }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_back.status(), StatusCode::CREATED);
    let delete_tar = app
        .clone()
        .oneshot(
            authed(Request::delete("/~acme/@acme/widget/-/widget-1.0.0.tgz/-rev/1"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_tar.status(), StatusCode::CREATED);
    assert!(!tmp.path().join("acme/@acme/widget/widget-1.0.0.tgz").exists());
    let delete_pkg = app
        .clone()
        .oneshot(
            authed(Request::delete("/~acme/@acme%2Fwidget/-rev/1")).body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_pkg.status(), StatusCode::CREATED);
    assert!(!tmp.path().join("acme/@acme/widget").exists());
}

/// Path-less responses resolved to a private registry carry the same
/// `Cache-Control: private, no-store` / `Vary: Authorization` headers the
/// `/~<name>/` surface applies — same content, same defense against a shared
/// HTTP cache. A public resolution stays cacheable.
#[tokio::test]
async fn pathless_private_registry_responses_carry_private_cache_headers() {
    let tmp = TempDir::new().unwrap();
    seed_hosted(&tmp.path().join("acme"), "@acme/widget");

    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.hosted.insert("acme".to_string(), hosted_with_access("acme", "alice"));
    config.registries = Registries::new(
        vec![("acme".to_string(), Registry::Hosted { patterns: vec![] })].into_iter().collect(),
        Some("acme".to_string()),
    );
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config, auth);

    for path in ["/@acme/widget", "/@acme%2Fwidget/1.0.0", "/-/package/@acme%2Fwidget/dist-tags"] {
        let response = app
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "GET {path}");
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).and_then(|value| value.to_str().ok()),
            Some("private, no-store"),
            "missing private cache header on {path}",
        );
        assert_eq!(
            response.headers().get(header::VARY).and_then(|value| value.to_str().ok()),
            Some("Authorization"),
            "missing Vary on {path}",
        );
    }

    // Control: the same content behind a public registry stays cacheable.
    let tmp_public = TempDir::new().unwrap();
    seed_hosted(&tmp_public.path().join("acme"), "@acme/widget");
    let mut config = config_for("http://127.0.0.1:1", tmp_public.path().to_path_buf());
    config.hosted.insert("acme".to_string(), hosted_with_access("acme", "$all"));
    config.registries = Registries::new(
        vec![("acme".to_string(), Registry::Hosted { patterns: vec![] })].into_iter().collect(),
        Some("acme".to_string()),
    );
    let app = router_with_auth(config, AuthState::in_memory());
    let response =
        app.oneshot(Request::get("/@acme/widget").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers().get(header::CACHE_CONTROL).is_none(),
        "a public path-less response must stay cacheable",
    );
}

/// A per-package ACL that denies anonymous callers makes the path-less
/// response vary by `Authorization` even when the source registry itself is
/// public, so it must carry the private-cache headers — otherwise a shared
/// HTTP cache could replay an authenticated 200 to an anonymous caller.
#[tokio::test]
async fn pathless_acl_gated_package_carries_private_cache_headers() {
    let tmp = TempDir::new().unwrap();
    seed_hosted(&tmp.path().join("acme"), "@acme/widget");

    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.hosted.insert("acme".to_string(), hosted_with_access("acme", "$all"));
    config.registries = Registries::new(
        vec![("acme".to_string(), Registry::Hosted { patterns: vec![] })].into_iter().collect(),
        Some("acme".to_string()),
    );
    config
        .hosted
        .get_mut("acme")
        .expect("hosted acme")
        .rules
        .push_rule(access_rule("@acme/widget", "$authenticated"));
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config, auth);

    let response = app
        .oneshot(
            Request::get("/@acme/widget")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).and_then(|value| value.to_str().ok()),
        Some("private, no-store"),
        "an ACL-gated path-less response must not be shared-cacheable",
    );
    assert_eq!(
        response.headers().get(header::VARY).and_then(|value| value.to_str().ok()),
        Some("Authorization"),
    );
}

#[test]
fn pipeline_viewer_renders_publisher_data_as_text() {
    let output = std::process::Command::new("node")
        .args(["--test", concat!(env!("CARGO_MANIFEST_DIR"), "/tests/pipeline_ui.mjs")])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "viewer regression: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[tokio::test]
async fn browse_paginates_hosted_packages_without_contacting_upstreams() {
    let tmp = TempDir::new().unwrap();
    seed_hosted(tmp.path(), "alpha");
    seed_hosted(tmp.path(), "beta");
    seed_hosted(tmp.path(), "gamma");
    seed_hosted(tmp.path(), "routed-to-upstream");
    seed_hosted(tmp.path(), "hidden");
    let mut upstream = mockito::Server::new_async().await;
    let search = upstream
        .mock("GET", "/-/v1/search")
        .match_query(mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.upstreams.get_mut("npmjs").unwrap().search = true;
    let mut hosted = hosted_with_access("", "$all");
    hosted.rules = PackageRules::new(
        vec![access_rule("hidden", "alice")],
        Some(AccessList::from_tokens(["$all"])),
    );
    config.hosted.insert("local".to_string(), hosted);
    config.registries = Registries::new(
        [
            (
                "local".to_string(),
                Registry::Hosted {
                    patterns: ["alpha", "beta", "gamma", "hidden"]
                        .into_iter()
                        .map(|name| PackagePattern::parse(name, Ecosystem::Npm).unwrap())
                        .collect(),
                },
            ),
            ("npmjs".to_string(), Registry::Upstream { patterns: vec![] }),
            (
                "main".to_string(),
                Registry::Router { sources: vec!["local".to_string(), "npmjs".to_string()] },
            ),
        ]
        .into_iter()
        .collect(),
        Some("main".to_string()),
    );
    let app = router(config);
    for base in ["", "/~main"] {
        for (from, name) in [(0, "alpha"), (1, "beta"), (2, "gamma")] {
            let response = app
                .clone()
                .oneshot(
                    Request::get(format!("{base}/-/v1/search?browse=true&size=1&from={from}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");
            let body = body_json(response.into_body()).await;
            assert_eq!(body["total"], 3);
            assert_eq!(body["objects"].as_array().unwrap().len(), 1);
            assert_eq!(body["objects"][0]["package"]["name"], name);
        }
    }
    let response = app
        .oneshot(Request::get("/-/v1/search?browse=true&from=3").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body = body_json(response.into_body()).await;
    assert_eq!(body["total"], 3);
    assert_eq!(body["objects"], json!([]));
    search.assert_async().await;
}

#[tokio::test]
async fn registry_directory_filters_private_registries_and_routing_details() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://example.invalid/secret-upstream", tmp.path().to_path_buf());
    config.hosted.insert("private".to_string(), hosted_with_access("private", "alice"));
    config.hosted.insert("crates".to_string(), hosted_with_access("crates", "$all"));
    config.registries = Registries::new(
        [
            (
                "private".to_string(),
                Registry::Hosted { patterns: vec![PackagePattern::Scope("secret".to_string())] },
            ),
            ("crates".to_string(), Registry::Hosted { patterns: vec![] }),
            ("npmjs".to_string(), Registry::Upstream { patterns: vec![] }),
            (
                "main".to_string(),
                Registry::Router {
                    sources: vec!["private".to_string(), "crates".to_string(), "npmjs".to_string()],
                },
            ),
        ]
        .into_iter()
        .collect(),
        Some("main".to_string()),
    )
    .with_ecosystem("crates", Ecosystem::Cargo);
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config, auth);
    for authenticated in [false, true] {
        let mut request = Request::builder().uri("/-/pnpr/v0/registries");
        if authenticated {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = app.clone().oneshot(request.body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["defaultRegistries"], json!({"npm": "main", "cargo": "main"}));
        assert_eq!(body["ecosystems"]["cargo"], json!({"available": true, "prefixed": true}));
        let entries = body["registries"].as_array().unwrap();
        let main = entries.iter().find(|registry| registry["name"] == "main").unwrap();
        assert_eq!(entries.iter().any(|registry| registry["name"] == "private"), authenticated);
        if authenticated {
            assert_eq!(main["sources"], json!(["private", "npmjs"]));
        } else {
            assert!(main["sources"].is_null());
            assert!(!String::from_utf8_lossy(&bytes).contains("secret"));
        }
        assert!(!String::from_utf8_lossy(&bytes).contains("example.invalid"));
    }
}
