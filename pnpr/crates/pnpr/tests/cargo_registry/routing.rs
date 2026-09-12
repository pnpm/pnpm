use super::{
    AuthState, Body, Ecosystem, Request, ServiceExt, StatusCode, TempDir, Value, body_bytes,
    cargo_config, common, crate_archive, find_file, header, json, metadata, publish_body,
    publish_request, router_with_auth, sha256_hex,
};

#[tokio::test]
async fn config_json_points_downloads_and_the_api_back_at_the_registry() {
    let tmp = TempDir::new().unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all"),
        AuthState::in_memory(),
    );

    let response = app
        .clone()
        .oneshot(Request::get("/cargo/index/config.json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let config: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(
        config,
        json!({ "dl": "http://pnpr.test/cargo/api/v1/crates", "api": "http://pnpr.test/cargo" }),
    );

    // A registry addressed by name advertises its own endpoint.
    for registry in ["crates", "main"] {
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/cargo/~{registry}/index/config.json"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{registry}");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "private, no-store");
        let config: Value =
            serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        assert_eq!(config["api"], format!("http://pnpr.test/cargo/~{registry}"));
    }

    let response = app
        .clone()
        .oneshot(Request::get("/npm/~main/-/whoami").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(Request::get("/npm/~crates/index/config.json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn search_hides_a_private_registry_from_an_anonymous_caller() {
    for url in ["/cargo/api/v1/crates?q=demo", "/cargo/api/v1/crates?browse=true"] {
        let tmp = TempDir::new().unwrap();
        let auth = AuthState::in_memory();
        let token = auth.tokens.issue("alice").await.unwrap();
        let app = router_with_auth(
            cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$authenticated"),
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

        let response =
            app.clone().oneshot(Request::get(url).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        assert_eq!(body, json!({ "crates": [], "meta": { "total": 0 } }));

        let response = app
            .oneshot(
                Request::get(url)
                    .header(header::AUTHORIZATION, &token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        assert_eq!(body["crates"][0]["name"], "demo");
    }
}

#[tokio::test]
async fn crate_names_are_case_insensitive_in_the_index_path() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$all"),
        auth,
    );
    let archive = crate_archive("Inflector", "0.11.4");
    let response = app
        .clone()
        .oneshot(publish_request(
            Some(&token),
            publish_body(&metadata("Inflector", "0.11.4"), &archive),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // `cargo` requests the lowercase path; the entry keeps the published case.
    let response = app
        .clone()
        .oneshot(Request::get("/cargo/index/in/fl/inflector").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let index = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    let line: Value = serde_json::from_str(index.lines().next().unwrap()).unwrap();
    assert_eq!(line["name"], "Inflector");

    for name in ["Inflector", "inflector", "INFLECTOR"] {
        let response = app
            .clone()
            .oneshot(
                Request::get(format!("/cargo/api/v1/crates/{name}/0.11.4/download"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response.into_body()).await, archive);
    }
}

#[tokio::test]
async fn private_hosted_registry_advertises_auth_required_and_masks_anonymous_reads() {
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), "http://upstream.invalid/", "$authenticated"),
        auth,
    );
    let response = app
        .clone()
        .oneshot(Request::get("/cargo/index/config.json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let config: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(config["auth-required"], true);

    let response = app
        .clone()
        .oneshot(publish_request(
            Some(&token),
            publish_body(&metadata("demo", "0.1.0"), &crate_archive("demo", "0.1.0")),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // The registry-level default masks the crate from anonymous callers.
    let response = app
        .clone()
        .oneshot(Request::get("/cargo/index/de/mo/demo").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = app
        .clone()
        .oneshot(
            Request::get("/cargo/api/v1/crates/demo/0.1.0/download").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // With the raw token `cargo` sends once `auth-required` is set, both serve.
    for path in ["/cargo/index/de/mo/demo", "/cargo/api/v1/crates/demo/0.1.0/download"] {
        let response = app
            .clone()
            .oneshot(
                Request::get(path)
                    .header(header::AUTHORIZATION, &token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
    }
}

#[tokio::test]
async fn proxies_the_sparse_index_and_verified_downloads_through_an_upstream() {
    let mut upstream = mockito::Server::new_async().await;
    let archive = crate_archive("serde", "1.0.0");
    let config_mock = upstream
        .mock("GET", "/config.json")
        .with_body(
            json!({ "dl": format!("{}/dl/{{crate}}/{{version}}", upstream.url()), "api": upstream.url() })
                .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let index_line = json!({
        "name": "serde",
        "vers": "1.0.0",
        "deps": [],
        "cksum": sha256_hex(&archive),
        "features": {},
        "yanked": false,
        "v": 1,
    });
    let index_mock = upstream
        .mock("GET", "/se/rd/serde")
        .with_body(format!("{index_line}\n"))
        .expect(1)
        .create_async()
        .await;
    let download_mock = upstream
        .mock("GET", "/dl/serde/1.0.0")
        .with_body(archive.clone())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), &upstream.url(), "$all"),
        AuthState::in_memory(),
    );

    // The index file is proxied verbatim.
    let response = app
        .clone()
        .oneshot(Request::get("/cargo/index/se/rd/serde").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let index = String::from_utf8(body_bytes(response.into_body()).await).unwrap();
    assert_eq!(index.trim(), index_line.to_string());

    // The download is bound to the index checksum and cached.
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(
                Request::get("/cargo/api/v1/crates/serde/1.0.0/download")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response.into_body()).await, archive);
    }
    config_mock.assert_async().await;
    index_mock.assert_async().await;
    download_mock.assert_async().await;
    let cache = tmp.path().join(".pnpr-cache");
    assert!(find_file(&cache, "serde-1.0.0.crate").is_some(), "download is cached");

    // An unknown crate is a definitive 404, and the cache holds nothing for it.
    let missing = upstream.mock("GET", "/no/pe/nope").with_status(404).create_async().await;
    let response = app
        .oneshot(Request::get("/cargo/index/no/pe/nope").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    missing.assert_async().await;
}

#[tokio::test]
async fn a_download_that_fails_the_index_checksum_is_never_cached() {
    let mut upstream = mockito::Server::new_async().await;
    let archive = crate_archive("serde", "1.0.0");
    upstream
        .mock("GET", "/config.json")
        .with_body(json!({ "dl": format!("{}/dl", upstream.url()) }).to_string())
        .create_async()
        .await;
    upstream
        .mock("GET", "/se/rd/serde")
        .with_body(format!(
            "{}\n",
            json!({
                "name": "serde",
                "vers": "1.0.0",
                "deps": [],
                "cksum": sha256_hex(b"something else"),
                "features": {},
                "yanked": false,
            }),
        ))
        .create_async()
        .await;
    // A `dl` template without markers gets `/{crate}/{version}/download` appended.
    upstream.mock("GET", "/dl/serde/1.0.0/download").with_body(archive).create_async().await;

    let tmp = TempDir::new().unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), &upstream.url(), "$all"),
        AuthState::in_memory(),
    );
    let response = app
        .oneshot(
            Request::get("/cargo/api/v1/crates/serde/1.0.0/download").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert!(axum::body::to_bytes(response.into_body(), usize::MAX).await.is_err());
    assert!(find_file(&tmp.path().join(".pnpr-cache"), "serde-1.0.0.crate").is_none());
}

#[tokio::test]
async fn cached_crates_follow_current_index_checksums_and_removals() {
    common::assert_cache_tracks_metadata(Ecosystem::Cargo).await;
}

#[tokio::test]
async fn upstream_sparse_index_rejects_invalid_utf8_before_serving_or_downloading() {
    let mut upstream = mockito::Server::new_async().await;
    let valid_index = json!({
        "name": "serde", "vers": "1.0.0", "cksum": "0".repeat(64),
        "deps": [], "features": {}, "extra": "X",
    })
    .to_string();
    let mut index = valid_index.as_bytes().to_vec();
    let invalid_byte = index.iter_mut().find(|byte| **byte == b'X').unwrap();
    *invalid_byte = 0xff;
    let index_mock =
        upstream.mock("GET", "/se/rd/serde").with_body(index).expect(2).create_async().await;
    let config_mock = upstream.mock("GET", "/config.json").expect(0).create_async().await;
    let tmp = TempDir::new().unwrap();
    let app = router_with_auth(
        cargo_config(tmp.path().to_path_buf(), &upstream.url(), "$all"),
        AuthState::in_memory(),
    );
    for path in ["/cargo/index/se/rd/serde", "/cargo/api/v1/crates/serde/1.0.0/download"] {
        let response =
            app.clone().oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }
    index_mock.assert_async().await;
    index_mock.remove_async().await;
    let corrected = upstream
        .mock("GET", "/se/rd/serde")
        .with_body(valid_index.clone())
        .expect(1)
        .create_async()
        .await;
    let response = app
        .clone()
        .oneshot(Request::get("/cargo/index/se/rd/serde").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, valid_index.as_bytes());
    let cached_index = find_file(&tmp.path().join(".pnpr-cache"), "package.json").unwrap();
    tokio::fs::write(cached_index, [0xff]).await.unwrap();
    let response = app
        .oneshot(Request::get("/cargo/index/se/rd/serde").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    corrected.assert_async().await;
    config_mock.assert_async().await;
}
