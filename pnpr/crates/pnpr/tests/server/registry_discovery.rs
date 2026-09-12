use super::{
    AccessList, AuthState, Body, Config, Ipv4Addr, PackagePattern, PackageRules, Registries,
    Registry, Request, ServiceExt, SocketAddr, SocketAddrV4, StatusCode, TempDir, Value,
    access_rule, body_bytes, body_json, config_for, fs, header, hosted_with_access, json, router,
    router_with_auth, seed_hosted, seed_hosted_with_maintainer, to_bytes,
};

#[tokio::test]
async fn search_paginates_visible_results_and_filters_by_maintainer() {
    let tmp = TempDir::new().unwrap();
    seed_hosted_with_maintainer(tmp.path(), "tool-a", "alice");
    seed_hosted_with_maintainer(tmp.path(), "tool-b", "bob");
    seed_hosted_with_maintainer(tmp.path(), "tool-c", "alice");
    let app = router(Config::static_serve(
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
        tmp.path().to_path_buf(),
    ));

    let page = app
        .clone()
        .oneshot(Request::get("/-/v1/search?text=tool&from=1&size=1").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let page = body_json(page.into_body()).await;
    assert_eq!(page["total"], json!(3));
    assert_eq!(page["objects"][0]["package"]["name"], json!("tool-b"));

    let maintained = app
        .oneshot(
            Request::get("/-/v1/search?text=maintainer%3Aalice&from=1&size=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let maintained = body_json(maintained.into_body()).await;
    assert_eq!(maintained["total"], json!(2));
    assert_eq!(maintained["objects"][0]["package"]["name"], json!("tool-c"));
}

#[tokio::test]
async fn organization_packages_are_available_on_both_registry_routes() {
    let tmp = TempDir::new().unwrap();
    seed_hosted(tmp.path(), "@acme/alpha");
    seed_hosted(tmp.path(), "@acme/beta");
    seed_hosted(tmp.path(), "@other/ignored");
    let app = router(Config::static_serve(
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
        tmp.path().to_path_buf(),
    ));

    for route in ["/-/org/acme/package", "/~main/-/org/acme/package"] {
        let response =
            app.clone().oneshot(Request::get(route).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let packages = body_json(response.into_body()).await;
        assert_eq!(packages["@acme/alpha"], json!("read"));
        assert_eq!(packages["@acme/beta"], json!("read"));
        assert!(packages.get("@other/ignored").is_none());
    }
}

#[tokio::test]
async fn opt_in_upstream_discovery_serves_search_and_organization_packages() {
    let mut upstream = mockito::Server::new_async().await;
    let search = upstream
        .mock("GET", "/-/v1/search")
        .match_query("text=remote&from=0&size=250")
        .match_header("authorization", mockito::Matcher::Missing)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "objects": [{
                    "package": { "name": "remote-package", "version": "1.0.0" },
                    "score": { "final": 1.0 },
                    "searchScore": 1.0,
                }],
                "total": 1,
            })
            .to_string(),
        )
        .create_async()
        .await;
    let org = upstream
        .mock("GET", "/-/org/acme/package")
        .match_header("authorization", mockito::Matcher::Missing)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "@acme/remote": "write" }).to_string())
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.upstreams.get_mut("npmjs").unwrap().search = true;
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config, auth);

    let response = app
        .clone()
        .oneshot(
            Request::get("/-/v1/search?text=remote&size=5")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let response = body_json(response.into_body()).await;
    assert_eq!(response["total"], json!(1));
    assert_eq!(response["objects"][0]["package"]["name"], json!("remote-package"));

    let response = app
        .oneshot(
            Request::get("/-/org/acme/package")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response.into_body()).await["@acme/remote"], json!("read"));
    search.assert_async().await;
    org.assert_async().await;
}

#[tokio::test]
async fn upstream_search_exhausts_results_to_return_an_exact_total() {
    let mut upstream = mockito::Server::new_async().await;
    let first = upstream
        .mock("GET", "/-/v1/search")
        .match_query("text=remote&from=0&size=250")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "objects": [
                    { "package": { "name": "remote-a" } },
                    { "package": { "name": "remote-b" } },
                ],
                "total": 3,
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let last = upstream
        .mock("GET", "/-/v1/search")
        .match_query("text=remote&from=2&size=250")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "objects": [{ "package": { "name": "remote-c" } }],
                "total": 3,
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.upstreams.get_mut("npmjs").unwrap().search = true;
    let app = router(config);

    let response = app
        .oneshot(Request::get("/-/v1/search?text=remote&size=1").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let response = body_json(response.into_body()).await;
    assert_eq!(response["total"], json!(3));
    assert_eq!(response["objects"][0]["package"]["name"], json!("remote-a"));
    first.assert_async().await;
    last.assert_async().await;
}

#[tokio::test]
async fn upstream_search_rejects_unbounded_offsets_and_result_sets() {
    let mut upstream = mockito::Server::new_async().await;
    let oversized = upstream
        .mock("GET", "/-/v1/search")
        .match_query("text=remote&from=0&size=250")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "objects": [], "total": 2_001 }).to_string())
        .expect(1)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.upstreams.get_mut("npmjs").unwrap().search = true;
    let app = router(config);

    let offset = app
        .clone()
        .oneshot(Request::get("/-/v1/search?text=remote&from=2001").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(offset.status(), StatusCode::BAD_REQUEST);

    let result_set = app
        .oneshot(Request::get("/-/v1/search?text=remote").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(result_set.status(), StatusCode::BAD_REQUEST);
    oversized.assert_async().await;
}

#[tokio::test]
async fn upstream_search_rejects_more_than_eight_short_pages() {
    let mut upstream = mockito::Server::new_async().await;
    let short_page = upstream
        .mock("GET", "/-/v1/search")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "objects": [{ "package": { "name": "repeated" } }],
                "total": 9,
            })
            .to_string(),
        )
        .expect(8)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.upstreams.get_mut("npmjs").unwrap().search = true;
    let app = router(config);

    let response = app
        .oneshot(Request::get("/-/v1/search?text=remote").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    short_page.assert_async().await;
}

#[tokio::test]
async fn registry_directory_hides_upstream_access_and_package_rule_metadata() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://example.invalid", tmp.path().to_path_buf());
    config.upstreams.get_mut("npmjs").unwrap().access = Some(AccessList::from_tokens(["alice"]));
    let mut hosted = hosted_with_access("public", "$all");
    hosted.rules = PackageRules::new(
        vec![access_rule("secret-package", "alice")],
        Some(AccessList::from_tokens(["$all"])),
    );
    config.hosted.insert("public".to_string(), hosted);
    config.registries = Registries::new(
        [
            (
                "public".to_string(),
                Registry::Hosted {
                    patterns: vec![PackagePattern::Exact("secret-package".to_string())],
                },
            ),
            ("npmjs".to_string(), Registry::Upstream { patterns: vec![] }),
        ]
        .into_iter()
        .collect(),
        Some("npmjs".to_string()),
    );
    let response = router(config)
        .oneshot(Request::builder().uri("/-/pnpr/v0/registries").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("secret-package"), "directory exposes a restricted package name");
    assert!(!text.contains("alice"), "directory exposes an access rule");
    assert_eq!(body["defaultRegistries"], json!({}));
    assert_eq!(body["registries"].as_array().unwrap().len(), 1);
    assert_eq!(body["registries"][0]["name"], "public");
    assert!(body["registries"][0]["patterns"].is_null());
    assert_eq!(body["ecosystems"]["npm"]["prefixed"], false);
}

#[tokio::test]
async fn registry_directory_describes_oci_only_named_endpoints() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("config.yaml");
    fs::write(&path, "storage: ./storage\nregistries:\n  oci:\n    internal: {type: hosted}\ndefaultRegistry:\n  oci: internal\n").unwrap();
    let config = Config::from_yaml(&path, "127.0.0.1:4873".parse().unwrap(), None).unwrap();
    let app = router(config);
    let response = app
        .clone()
        .oneshot(Request::get("/-/pnpr/v0/registries").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let directory: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(
        directory["ecosystems"]["oci"],
        json!({"available": true, "prefixed": false, "namedPrefixed": false}),
    );
    for path in ["/~internal/v2/", "/v2/"] {
        let response =
            app.clone().oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}
