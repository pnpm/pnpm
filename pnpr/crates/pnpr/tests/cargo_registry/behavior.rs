use super::{
    AuthState, Body, Request, ServiceExt, StatusCode, TempDir, Value, body_bytes, cargo_config,
    crate_archive, header, json, metadata, publish_body, publish_request, router_with_auth,
};

#[tokio::test]
async fn search_lists_hosted_crates_by_newest_version_and_description() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all"),
        auth,
    );
    for (name, version) in [("demo", "0.1.0"), ("demo", "1.2.0"), ("inflector", "0.11.4")] {
        let response = app
            .clone()
            .oneshot(publish_request(
                Some(&token),
                publish_body(&metadata(name, version), &crate_archive(name, version)),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{name}@{version}");
    }

    let search = |app: axum::Router, query: &str| {
        let uri = format!("/cargo/api/v1/crates?{query}");
        async move {
            let response =
                app.oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            serde_json::from_slice::<Value>(&body_bytes(response.into_body()).await).unwrap()
        }
    };

    let body = search(app.clone(), "q=demo&per_page=10").await;
    assert_eq!(
        body,
        json!({
            "crates": [{ "name": "demo", "description": "A demo crate", "max_version": "1.2.0" }],
            "meta": { "total": 1 },
        }),
    );

    let body = search(app.clone(), "q=flect").await;
    assert_eq!(body["crates"][0]["name"], "inflector");
    assert_eq!(body["meta"]["total"], 1);

    let body = search(app.clone(), "q=o&per_page=1").await;
    assert_eq!(body["crates"].as_array().unwrap().len(), 1);
    assert_eq!(body["crates"][0]["name"], "demo");
    assert_eq!(body["meta"]["total"], 2);
    let body = search(app.clone(), "q=o&per_page=1&page=2").await;
    assert_eq!(body["crates"].as_array().unwrap().len(), 1);
    assert_eq!(body["crates"][0]["name"], "inflector");

    for (page, name) in [(1, "demo"), (2, "inflector")] {
        let body = search(app.clone(), &format!("browse=true&per_page=1&page={page}")).await;
        assert_eq!(body["meta"]["total"], 2);
        assert_eq!(body["crates"].as_array().unwrap().len(), 1);
        assert_eq!(body["crates"][0]["name"], name);
    }
    let body = search(app.clone(), "browse=true&per_page=1&page=3").await;
    assert_eq!(body, json!({ "crates": [], "meta": { "total": 2 } }));

    // A query that names nothing is not a request to dump the registry.
    let body = search(app.clone(), "q=nothing-matches-this").await;
    assert_eq!(body, json!({ "crates": [], "meta": { "total": 0 } }));
    let body = search(app.clone(), "per_page=10").await;
    assert_eq!(body, json!({ "crates": [], "meta": { "total": 0 } }));

    // Results depend on the caller, so they must never be shared-cached.
    let response = app
        .oneshot(Request::get("/cargo/api/v1/crates?q=demo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");
    assert_eq!(response.headers()[header::VARY], "Authorization");
}

#[tokio::test]
async fn cargo_advertises_auth_for_package_specific_private_access() {
    use pnpr::{AccessList, Ecosystem, PackagePattern, PackageRule};
    let tmp = TempDir::new().unwrap();
    let mut config = cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all");
    config.hosted.get_mut("crates").unwrap().rules.push_rule(PackageRule {
        pattern: PackagePattern::parse("demo", Ecosystem::Npm).unwrap(),
        access: Some(AccessList::from_tokens(["$authenticated"])),
        publish: None,
        unpublish: None,
    });
    let app = router_with_auth(config, AuthState::in_memory());
    for prefix in ["/cargo", "/cargo/~crates", "/cargo/~main"] {
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("{prefix}/index/config.json")).body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let config: Value =
            serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        assert_eq!(config["auth-required"], true);
    }
}
