use super::{
    AuthState, Body, HeaderValue, PackageRules, Registries, Registry, Request, ServiceExt,
    StatusCode, TempDir, Value, access_rule, body_bytes, config_for, header,
    integrity_addressed_tarball_path, json, router_config, router_with_auth, sha512_integrity,
    upstream_endpoint_config,
};

#[tokio::test]
async fn upstream_endpoint_preserves_and_serves_revision_tarballs() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"immutable-revision-tarball";
    let integrity_text = sha512_integrity(bytes);
    let integrity = integrity_text.parse().unwrap();
    let revision_path = integrity_addressed_tarball_path(&integrity).unwrap();
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": { "name": "foo", "version": "1.0.0", "dist": {
            "tarball": format!("{}/{}", upstream.url(), revision_path),
            "integrity": integrity_text.clone(),
            "revision": 2,
            "revisions": [{
                "revision": 2,
                "integrity": integrity_text,
                "tarball": format!("{}/{}", upstream.url(), revision_path),
                "manifest": {},
            }],
        } } },
    });
    let packument_mock = upstream
        .mock("GET", "/foo")
        .match_header("x-upstream-auth", "secret")
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;
    let tarball_mock = upstream
        .mock("GET", format!("/{revision_path}").as_str())
        .match_header("x-upstream-auth", "secret")
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = upstream_endpoint_config(&upstream.url(), tmp.path().to_path_buf(), "alice");
    config
        .upstreams
        .get_mut("npmjs")
        .unwrap()
        .headers
        .insert("x-upstream-auth", HeaderValue::from_static("secret"));
    config.registries = Registries::new(
        std::iter::once(("npmjs".to_string(), Registry::Upstream { patterns: Vec::new() }))
            .collect(),
        Some("npmjs".to_string()),
    );
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config, auth);
    let authorization = format!("Bearer {token}");

    let response = app
        .clone()
        .oneshot(
            Request::get("/~npmjs/foo")
                .header(header::AUTHORIZATION, &authorization)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(body["versions"]["1.0.0"]["dist"]["revision"], 2);
    assert_eq!(
        body["versions"]["1.0.0"]["dist"]["tarball"],
        format!("http://example.test/~npmjs/{revision_path}"),
    );

    for path in [format!("/~npmjs/{revision_path}"), format!("/{revision_path}")] {
        let response = app
            .clone()
            .oneshot(
                Request::get(&path)
                    .header(header::AUTHORIZATION, &authorization)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(response.headers().get(header::CACHE_CONTROL).unwrap(), "private, no-store");
        assert_eq!(body_bytes(response.into_body()).await, bytes);
    }

    packument_mock.assert_async().await;
    tarball_mock.assert_async().await;
}

#[tokio::test]
async fn upstream_revision_tarball_does_not_follow_redirects() {
    let mut upstream = mockito::Server::new_async().await;
    let integrity = sha512_integrity(b"expected revision tarball").parse().unwrap();
    let revision_path = integrity_addressed_tarball_path(&integrity).unwrap();
    let redirect = upstream
        .mock("GET", format!("/{revision_path}").as_str())
        .with_status(302)
        .with_header("location", "/redirected.tgz")
        .expect(1)
        .create_async()
        .await;
    let redirected = upstream.mock("GET", "/redirected.tgz").expect(0).create_async().await;

    let tmp = TempDir::new().unwrap();
    let config = upstream_endpoint_config(&upstream.url(), tmp.path().to_path_buf(), "alice");
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config, auth);
    let response = app
        .oneshot(
            Request::get(format!("/~npmjs/{revision_path}"))
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
async fn revision_tarballs_require_a_concrete_registry_without_package_access_rules() {
    let mut upstream = mockito::Server::new_async().await;
    let integrity = sha512_integrity(b"unreachable revision tarball").parse().unwrap();
    let revision_path = integrity_addressed_tarball_path(&integrity).unwrap();
    let tarball =
        upstream.mock("GET", format!("/{revision_path}").as_str()).expect(0).create_async().await;

    let tmp = TempDir::new().unwrap();
    let router_app = router_with_auth(
        router_config(&upstream.url(), &upstream.url(), tmp.path().join("router")),
        AuthState::in_memory(),
    );
    for path in [format!("/~main/{revision_path}"), format!("/{revision_path}")] {
        let response = router_app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    let mut config = config_for(&upstream.url(), tmp.path().join("package-access"));
    config.upstreams.get_mut("npmjs").unwrap().rules =
        PackageRules::new(vec![access_rule("restricted", "$authenticated")], None);
    let response = router_with_auth(config, AuthState::in_memory())
        .oneshot(Request::get(format!("/~npmjs/{revision_path}")).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    tarball.assert_async().await;
}
