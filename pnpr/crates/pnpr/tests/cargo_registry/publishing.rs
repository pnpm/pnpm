use super::{
    AuthState, Body, Request, ServiceExt, StatusCode, TempDir, Value, body_bytes, cargo_config,
    crate_archive, fabricate_crashed_crate_publish, header, json, metadata, publish_body,
    publish_request, recover_publish_journal, registry_groups, router_with_auth, sha256_hex,
};

#[tokio::test]
async fn publish_then_resolve_and_download_a_hosted_crate() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all"),
        auth,
    );
    let archive = crate_archive("demo", "0.1.0");

    let response = app
        .clone()
        .oneshot(publish_request(Some(&token), publish_body(&metadata("demo", "0.1.0"), &archive)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(body["warnings"]["other"], json!([]));

    // The sparse-index file: one JSON line per version.
    let response = app
        .clone()
        .oneshot(Request::get("/cargo/index/de/mo/demo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    // A public crate through the default target stays cacheable.
    assert!(response.headers().get(header::CACHE_CONTROL).is_none());
    let index = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    let lines: Vec<Value> = index.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["name"], "demo");
    assert_eq!(lines[0]["vers"], "0.1.0");
    assert_eq!(lines[0]["cksum"], sha256_hex(&archive));
    assert_eq!(lines[0]["yanked"], false);
    assert_eq!(lines[0]["deps"][0]["name"], "serde");
    assert_eq!(lines[0]["deps"][0]["req"], "^1");

    // The archive is served byte-for-byte from the download endpoint.
    let response = app
        .clone()
        .oneshot(
            Request::get("/cargo/api/v1/crates/demo/0.1.0/download").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, archive);

    // Storage layout: the hosted org namespace, keyed by the lowercase name.
    assert!(tmp.path().join("crates/demo/demo-0.1.0.crate").is_file());
    assert!(tmp.path().join("crates/demo/package.json").is_file());
    assert!(std::fs::read_dir(tmp.path().join(".pnpr-journal")).unwrap().next().is_none());

    // The crate is reachable at its one sparse-index path only.
    for wrong in ["/cargo/index/3/d/demo", "/cargo/index/de/mo/Demo", "/cargo/index/DE/MO/demo"] {
        let response =
            app.clone().oneshot(Request::get(wrong).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{wrong}");
    }
    // The same crate through the named registry, caller-scoped.
    let response = app
        .clone()
        .oneshot(Request::get("/cargo/~crates/index/de/mo/demo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");
    // npm-shaped paths mean nothing on the Cargo surface.
    let response = app
        .clone()
        .oneshot(Request::get("/cargo/demo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = app
        .oneshot(
            Request::get("/cargo/api/v1/crates/demo/0.2.0/download").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn publish_requires_a_token_and_refuses_duplicates_and_bad_archives() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all"),
        auth,
    );
    let archive = crate_archive("demo", "0.1.0");
    let body = publish_body(&metadata("demo", "0.1.0"), &archive);

    // Anonymous: 401 in the crates API's JSON error shape.
    let response = app.clone().oneshot(publish_request(None, body.clone())).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let errors: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert!(errors["errors"][0]["detail"].is_string(), "{errors}");

    let response = app.clone().oneshot(publish_request(Some(&token), body.clone())).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Re-publishing the same version is refused.
    let response = app.clone().oneshot(publish_request(Some(&token), body)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let errors: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert!(
        errors["errors"][0]["detail"].as_str().unwrap().contains("already uploaded"),
        "{errors}",
    );

    // The archive must hold the crate the metadata names.
    let mismatched = publish_body(&metadata("demo", "0.2.0"), &crate_archive("other", "0.2.0"));
    let response = app.clone().oneshot(publish_request(Some(&token), mismatched)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(!tmp.path().join("crates/demo/demo-0.2.0.crate").exists());

    // A name the hosted registry does not claim routes to the upstream, where
    // nothing can be published.
    let unclaimed = publish_body(&metadata("serde", "1.0.0"), &crate_archive("serde", "1.0.0"));
    let response = app.clone().oneshot(publish_request(Some(&token), unclaimed)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let errors: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert!(
        errors["errors"][0]["detail"].as_str().unwrap().contains("upstream registry"),
        "{errors}",
    );

    // A malformed body is a 400, not a 500.
    let response = app.oneshot(publish_request(Some(&token), vec![1, 2, 3])).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn yank_and_unyank_flip_the_index_entry() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all"),
        auth,
    );
    let response = app
        .clone()
        .oneshot(publish_request(
            Some(&token),
            publish_body(&metadata("demo", "0.1.0"), &crate_archive("demo", "0.1.0")),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let yanked_flag = |app: axum::Router| async move {
        let response = app
            .oneshot(Request::get("/cargo/index/de/mo/demo").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let index = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
        let line: Value = serde_json::from_str(index.lines().next().unwrap()).unwrap();
        line["yanked"].as_bool().unwrap()
    };

    // Anonymous yank is refused.
    let response = app
        .clone()
        .oneshot(
            Request::delete("/cargo/api/v1/crates/demo/0.1.0/yank").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(!yanked_flag(app.clone()).await);

    let response = app
        .clone()
        .oneshot(
            Request::delete("/cargo/api/v1/crates/demo/0.1.0/yank")
                .header(header::AUTHORIZATION, &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(yanked_flag(app.clone()).await);

    // A yanked version stays downloadable, as on crates.io.
    let response = app
        .clone()
        .oneshot(
            Request::get("/cargo/api/v1/crates/demo/0.1.0/download").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::put("/cargo/api/v1/crates/demo/0.1.0/unyank")
                .header(header::AUTHORIZATION, &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!yanked_flag(app.clone()).await);

    // An unknown version is a 404.
    let response = app
        .oneshot(
            Request::delete("/cargo/api/v1/crates/demo/9.9.9/yank")
                .header(header::AUTHORIZATION, &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_publishers_description_cannot_grow_a_search_response() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all"),
        auth,
    );
    let mut published = metadata("demo", "0.1.0");
    published["description"] = json!("d".repeat(pnpr_cargo::MAX_DESCRIPTION_LEN + 500));
    let response = app
        .clone()
        .oneshot(publish_request(
            Some(&token),
            publish_body(&published, &crate_archive("demo", "0.1.0")),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(Request::get("/cargo/api/v1/crates?q=demo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(
        body["crates"][0]["description"].as_str().unwrap().len(),
        pnpr_cargo::MAX_DESCRIPTION_LEN,
    );
}

#[tokio::test]
async fn search_reports_the_newest_unyanked_release() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all"),
        auth,
    );
    for version in ["0.1.0", "1.2.0"] {
        let response = app
            .clone()
            .oneshot(publish_request(
                Some(&token),
                publish_body(&metadata("demo", version), &crate_archive("demo", version)),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{version}");
    }
    let response = app
        .clone()
        .oneshot(
            Request::delete("/cargo/api/v1/crates/demo/1.2.0/yank")
                .header(header::AUTHORIZATION, &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(Request::get("/cargo/api/v1/crates?q=demo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(body["crates"][0]["max_version"], "0.1.0");
}

/// Both halves land, so the store never holds an archive that no index file
/// mentions.
#[tokio::test]
async fn a_crashed_publish_is_completed_on_startup() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let config = cargo_config(storage.clone(), "http://upstream.invalid/", "$all");
    let archive = crate_archive("demo", "0.1.0");
    let tmp_path = fabricate_crashed_crate_publish(&storage, &archive);

    recover_publish_journal(&config).await.unwrap();

    assert!(!tmp_path.exists(), "the staged archive should be promoted away");
    assert!(std::fs::read_dir(storage.join(".pnpr-journal")).unwrap().next().is_none());
    let app = router_with_auth(config, AuthState::in_memory());
    let response = app
        .clone()
        .oneshot(Request::get("/cargo/index/de/mo/demo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let index = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    let entry: Value = serde_json::from_str(index.lines().next().unwrap()).unwrap();
    assert_eq!(entry["vers"], "0.1.0");
    assert_eq!(entry["cksum"], sha256_hex(&archive));
    let response = app
        .oneshot(
            Request::get("/cargo/api/v1/crates/demo/0.1.0/download").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, archive);
}

/// Applying a sealed transaction adds its version to the document as it
/// stands rather than to the one the crashed publish read.
#[tokio::test]
async fn a_crashed_publish_keeps_what_was_published_while_it_was_down() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let config = cargo_config(storage.clone(), "http://upstream.invalid/", "$all");
    let app = router_with_auth(config.clone(), auth);
    let archive = crate_archive("demo", "0.2.0");
    let response = app
        .oneshot(publish_request(Some(&token), publish_body(&metadata("demo", "0.2.0"), &archive)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    fabricate_crashed_crate_publish(&storage, &crate_archive("demo", "0.1.0"));

    recover_publish_journal(&config).await.unwrap();

    let app = router_with_auth(config, AuthState::in_memory());
    let response = app
        .oneshot(Request::get("/cargo/index/de/mo/demo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let index = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    let versions: Vec<Value> = index
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["vers"].clone())
        .collect();
    assert_eq!(versions, vec!["0.2.0", "0.1.0"]);
}

#[tokio::test]
async fn grouped_cargo_publish_stays_out_of_same_named_npm_registry() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(registry_groups::grouped_config(tmp.path(), "$all"), auth);
    let archive = crate_archive("demo", "0.1.0");
    let request = Request::put("/cargo/~internal/api/v1/crates/new")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(publish_body(&metadata("demo", "0.1.0"), &archive)))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    for base in ["/cargo/~internal", "/cargo/~main", "/cargo"] {
        let response = app
            .clone()
            .oneshot(Request::get(format!("{base}/index/de/mo/demo")).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{base}");
        let entry: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        assert_eq!(entry["vers"], "0.1.0");
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("{base}/api/v1/crates/demo/0.1.0/download"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{base}");
        assert_eq!(body_bytes(response.into_body()).await, archive);
    }
    for path in
        ["/npm/~internal/demo", "/npm/~main/demo", "/npm/demo", "/pypi/~internal/simple/demo/"]
    {
        let response =
            app.clone().oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}
