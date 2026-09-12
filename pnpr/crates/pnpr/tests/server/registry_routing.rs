use super::{
    AuthState, Body, Ecosystem, MaxUsers, PackagePattern, Registries, Registry, Request,
    ServiceExt, StatusCode, TempDir, assert_grouped_ecosystem, body_bytes, body_json, config_for,
    header, hosted_with_access, json, mock_package, mock_packument_for_tarball,
    read_registry_directory, registry_groups, router, router_config, router_with_auth,
};

#[tokio::test]
async fn scoped_packument_is_served() {
    let mut upstream = mockito::Server::new_async().await;
    let packument = json!({ "name": "@types/node", "versions": {} });
    let mock = upstream
        .mock("GET", "/@types/node")
        .with_status(200)
        .with_body(packument.to_string())
        .expect(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let response =
        app.oneshot(Request::get("/@types/node").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    mock.assert_async().await;
}

/// npm spells a scoped package's name either as two literal path segments or
/// as one percent-encoded segment, so the same package has several addresses.
#[tokio::test]
async fn every_address_of_one_scoped_package_reaches_it() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"scoped-address-bytes";
    let _packument_mock =
        mock_packument_for_tarball(&mut upstream, "@types/node", "20.0.0", bytes).await;
    let _tarball_mock = upstream
        .mock("GET", "/@types/node/-/node-20.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));
    let get = |path: &str| {
        let app = app.clone();
        let path = path.to_string();
        async move {
            app.oneshot(Request::get(path.as_str()).body(Body::empty()).unwrap()).await.unwrap()
        }
    };

    for path in ["/@types/node", "/@types%2Fnode", "/~npmjs/@types/node", "/~npmjs/@types%2Fnode"] {
        let response = get(path).await;
        assert_eq!(response.status(), StatusCode::OK, "GET {path}");
        assert_eq!(body_json(response.into_body()).await["name"], "@types/node", "GET {path}");
    }

    for path in [
        "/@types/node/20.0.0",
        "/@types%2Fnode/20.0.0",
        "/~npmjs/@types/node/20.0.0",
        "/~npmjs/@types%2Fnode/20.0.0",
    ] {
        let response = get(path).await;
        assert_eq!(response.status(), StatusCode::OK, "GET {path}");
        assert_eq!(body_json(response.into_body()).await["version"], "20.0.0", "GET {path}");
    }

    for path in [
        "/@types/node/-/node-20.0.0.tgz",
        "/@types%2Fnode/-/node-20.0.0.tgz",
        "/~npmjs/@types/node/-/node-20.0.0.tgz",
        "/~npmjs/@types%2Fnode/-/node-20.0.0.tgz",
    ] {
        let response = get(path).await;
        assert_eq!(response.status(), StatusCode::OK, "GET {path}");
        assert_eq!(body_bytes(response.into_body()).await, bytes, "GET {path}");
    }
}

#[tokio::test]
async fn invalid_package_name_returns_bad_request() {
    let upstream = mockito::Server::new_async().await;
    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    // `.hidden` trips the dot-prefix rejection in `CanonicalPackageName::parse`.
    let response =
        app.oneshot(Request::get("/.hidden").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn router_routes_each_package_to_its_declared_source() {
    let mut npmjs = mockito::Server::new_async().await;
    let mut corp = mockito::Server::new_async().await;
    let lodash_bytes = mock_package(&mut npmjs, "lodash", "public").await;
    let corp_bytes = mock_package(&mut corp, "@corp/secret", "private").await;

    let tmp = TempDir::new().unwrap();
    let config = router_config(&npmjs.url(), &corp.url(), tmp.path().to_path_buf());
    let app = router_with_auth(config, AuthState::in_memory());

    // `@corp/*` resolves to the corp upstream, authoritatively.
    let corp_tar = app
        .clone()
        .oneshot(
            Request::get("/~main/@corp/secret/-/secret-1.0.0.tgz").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(corp_tar.status(), StatusCode::OK);
    assert_eq!(body_bytes(corp_tar.into_body()).await, corp_bytes);

    // Everything else falls to the npmjs upstream via the `**` route.
    let public_tar = app
        .clone()
        .oneshot(Request::get("/~main/lodash/-/lodash-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(public_tar.status(), StatusCode::OK);
    assert_eq!(body_bytes(public_tar.into_body()).await, lodash_bytes);

    // The path-less base aliases the `main` router (the default target), so the
    // bare host routes identically.
    let bare = app
        .oneshot(Request::get("/lodash/-/lodash-1.0.0.tgz").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(bare.status(), StatusCode::OK);
    assert_eq!(body_bytes(bare.into_body()).await, lodash_bytes);
}

#[tokio::test]
async fn router_not_found_does_not_fall_through_to_public() {
    // A router whose only source claims the private `@corp/*` scope. A public
    // name no source claims must be a definitive 404 — never served from a
    // public origin — which is the dependency-confusion vector closed by
    // construction. The same namespace bound holds on the registry's own URL:
    // `/~corp/lodash` is a 404 answered before the upstream (and its
    // server-owned credential) is consulted.
    let mut corp = mockito::Server::new_async().await;
    let _ = mock_package(&mut corp, "@corp/secret", "private").await;
    let off_pattern_fetch = corp.mock("GET", "/lodash").expect(0).create_async().await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    let mut corp_upstream =
        config.upstreams.get("npmjs").expect("default `npmjs` upstream").clone();
    corp_upstream.url = corp.url();
    config.upstreams.insert("corp".to_string(), corp_upstream);
    let graph = vec![
        (
            "corp".to_string(),
            Registry::Upstream {
                patterns: vec![PackagePattern::parse("@corp/*", Ecosystem::Npm).unwrap()],
            },
        ),
        ("main".to_string(), Registry::Router { sources: vec!["corp".to_string()] }),
    ];
    config.registries = Registries::new(graph.into_iter().collect(), Some("main".to_string()));
    let app = router_with_auth(config, AuthState::in_memory());

    // The claimed private scope still serves.
    let matched = app
        .clone()
        .oneshot(Request::get("/~main/@corp/secret").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(matched.status(), StatusCode::OK);

    // An unclaimed public name is a definitive not-found, not a fall-through.
    let unclaimed = app
        .clone()
        .oneshot(Request::get("/~main/lodash").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(unclaimed.status(), StatusCode::NOT_FOUND);

    // Addressing the upstream registry directly is bounded the same way, without
    // an upstream fetch.
    let direct =
        app.oneshot(Request::get("/~corp/lodash").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(direct.status(), StatusCode::NOT_FOUND);
    off_pattern_fetch.assert_async().await;
}

#[tokio::test]
async fn router_unavailable_source_errors_not_404() {
    // The matched source is down. The router must surface an *error*, never a
    // 404 — reporting "not found" would let a downstream proxy or the client's
    // next-configured registry fall through to a different origin.
    let tmp = TempDir::new().unwrap();
    // Point `corp` at a closed port so every fetch is a transport failure.
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    let mut corp_upstream =
        config.upstreams.get("npmjs").expect("default `npmjs` upstream").clone();
    corp_upstream.url = "http://127.0.0.1:1".to_string();
    config.upstreams.insert("corp".to_string(), corp_upstream);
    let graph = vec![
        (
            "corp".to_string(),
            Registry::Upstream {
                patterns: vec![PackagePattern::parse("@corp/*", Ecosystem::Npm).unwrap()],
            },
        ),
        ("main".to_string(), Registry::Router { sources: vec!["corp".to_string()] }),
    ];
    config.registries = Registries::new(graph.into_iter().collect(), Some("main".to_string()));
    let app = router_with_auth(config, AuthState::in_memory());

    let response = app
        .oneshot(Request::get("/~main/@corp/secret").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_ne!(response.status(), StatusCode::NOT_FOUND);
    assert!(response.status().is_server_error(), "expected 5xx, got {}", response.status());
}

/// A programmatically-built registry graph gets the same fail-closed
/// validation as a YAML load: server construction folds every upstream into
/// the graph and rejects an invalid graph instead of serving it.
#[tokio::test]
async fn building_the_server_rejects_an_invalid_programmatic_registry_graph() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.registries = Registries::new(
        vec![("main".to_string(), Registry::Router { sources: vec!["ghost".to_string()] })]
            .into_iter()
            .collect(),
        None,
    );
    let err =
        pnpr::try_router(config).expect_err("an unknown router source must fail server startup");
    assert!(err.to_string().contains("ghost"), "unexpected error: {err}");
}

/// A concrete registry declared in the graph without its serving config
/// would answer every request not-found at runtime; server construction
/// rejects the mismatch instead.
#[tokio::test]
async fn building_the_server_rejects_a_concrete_registry_without_serving_config() {
    let tmp = TempDir::new().unwrap();

    // A hosted graph entry with no hosted-table row.
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.registries = Registries::new(
        vec![("ghost-org".to_string(), Registry::Hosted { patterns: vec![] })]
            .into_iter()
            .collect(),
        None,
    );
    let err = pnpr::try_router(config).expect_err("a backing-less hosted registry must fail");
    assert!(err.to_string().contains("ghost-org"), "unexpected error: {err}");

    // An upstream graph entry with no upstream serving config.
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.registries = Registries::new(
        vec![("phantom".to_string(), Registry::Upstream { patterns: vec![] })]
            .into_iter()
            .collect(),
        None,
    );
    let err = pnpr::try_router(config).expect_err("a backing-less upstream registry must fail");
    assert!(err.to_string().contains("phantom"), "unexpected error: {err}");
}

/// One name cannot identify two different origins: serving config left under
/// a name the graph declares as a different kind fails construction,
/// mirroring the YAML collision rejection.
#[tokio::test]
async fn building_the_server_rejects_a_name_shared_by_two_registry_kinds() {
    let tmp = TempDir::new().unwrap();

    // Upstream serving config under a name the graph declares as hosted.
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.registries = Registries::new(
        vec![("npmjs".to_string(), Registry::Hosted { patterns: vec![] })].into_iter().collect(),
        None,
    );
    let err = pnpr::try_router(config).expect_err("an upstream/hosted name collision must fail");
    assert!(err.to_string().contains("collides"), "unexpected error: {err}");

    // A hosted serving row under a name the graph declares as a router.
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.hosted.insert("corp".to_string(), hosted_with_access("corp", "$all"));
    config.registries = Registries::new(
        vec![("corp".to_string(), Registry::Router { sources: vec!["npmjs".to_string()] })]
            .into_iter()
            .collect(),
        None,
    );
    let err = pnpr::try_router(config).expect_err("a hosted/router name collision must fail");
    assert!(err.to_string().contains("collides"), "unexpected error: {err}");
}

/// The identity endpoints — adduser/login, whoami, profile, token list,
/// logout, token revoke — are global, so they are served under *any*
/// `/~<prefix>/` without consulting the registry table: clients derive these
/// URLs from their configured registry URL, so `pnpm login --registry
/// .../~corp/` must work even when nothing named `corp` is mounted. Skipping
/// the lookup also keeps them from becoming an existence oracle for private
/// registry names (a defined and an undefined registry must not answer
/// differently).
#[tokio::test]
async fn identity_endpoints_are_served_under_any_registry_prefix() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.auth.htpasswd.max_users = MaxUsers::Unlimited;
    let app = router(config);

    // Login against a registry prefix that is NOT a defined registry.
    let registration = json!({
        "_id": "org.couchdb.user:alice",
        "name": "alice",
        "password": "secret",
        "email": "alice@example.test",
        "type": "user",
        "roles": [],
    });
    let login = || {
        Request::put("/~corp/-/user/org.couchdb.user:alice")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&registration).unwrap()))
            .unwrap()
    };
    let logged_in = app.clone().oneshot(login()).await.unwrap();
    assert_eq!(logged_in.status(), StatusCode::CREATED);
    let token = body_json(logged_in.into_body()).await["token"].as_str().unwrap().to_string();

    // whoami, profile, and token list answer under the same prefix.
    for path in ["/~corp/-/whoami", "/~corp/-/npm/v1/user", "/~corp/-/npm/v1/tokens"] {
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
    }

    // Token revocation by key (`npm token revoke`) works under the prefix.
    let tokens = app
        .clone()
        .oneshot(
            Request::get("/~corp/-/npm/v1/tokens")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let key = body_json(tokens.into_body()).await["objects"][0]["key"]
        .as_str()
        .expect("token listing has a key")
        .to_string();
    let revoke = app
        .clone()
        .oneshot(
            Request::delete(format!("/~corp/-/npm/v1/tokens/token/{key}").as_str())
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(revoke.status().is_success(), "token revoke got {}", revoke.status());

    // Log back in and out (`npm logout` sends the raw token in the URL);
    // afterwards the token no longer authenticates.
    let logged_in = app.clone().oneshot(login()).await.unwrap();
    assert_eq!(logged_in.status(), StatusCode::CREATED);
    let token = body_json(logged_in.into_body()).await["token"].as_str().unwrap().to_string();
    let logout = app
        .clone()
        .oneshot(
            Request::delete(format!("/~corp/-/user/token/{token}").as_str())
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(logout.status().is_success(), "logout got {}", logout.status());
    let stale = app
        .clone()
        .oneshot(
            Request::get("/~corp/-/whoami")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::UNAUTHORIZED);

    // No existence oracle: anonymous whoami answers 401 identically whether
    // or not the prefix names a real registry (`npmjs` is defined by config_for).
    for path in ["/~corp/-/whoami", "/~npmjs/-/whoami"] {
        let response =
            app.clone().oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "GET {path}");
    }
}

/// The account and staging endpoints answer under a `/~<name>/` prefix only.
/// Letting *any* first segment reach them would expose them at as many
/// addresses as a client cares to invent.
#[tokio::test]
async fn a_first_segment_that_is_not_a_tilde_prefix_does_not_reach_the_account_endpoints() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.auth.htpasswd.max_users = MaxUsers::Unlimited;
    let app = router(config);

    for path in ["/~/-/whoami", "/corp/-/npm/v1/tokens", "/~/-/npm/v1/user"] {
        let response =
            app.clone().oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "GET {path}");
    }

    // `/corp/-/whoami` is a well-formed tarball address — package `corp`,
    // file `whoami` — so it reads through the registry graph instead of
    // answering as whoami. The configured upstream is unreachable, which is
    // what a package read of it reports.
    let tarball_shaped =
        app.oneshot(Request::get("/corp/-/whoami").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(tarball_shaped.status(), StatusCode::SERVICE_UNAVAILABLE);
}

/// The scoped addresses spend two path segments on the package name, so their
/// first segment has to be a scope. The configured upstream is unreachable, so
/// a 404 rather than a 503 also shows the request was turned away before any
/// registry lookup.
#[tokio::test]
async fn a_scoped_address_whose_first_segment_is_not_a_scope_is_not_found() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.auth.htpasswd.max_users = MaxUsers::Unlimited;
    let app = router(config);

    for (method, path) in [
        ("GET", "/notascope/widget/1.0.0"),
        ("GET", "/notascope/widget/-/widget-1.0.0.tgz"),
        ("GET", "/~npmjs/notascope/widget/1.0.0"),
        ("GET", "/~npmjs/notascope/widget/-/widget-1.0.0.tgz"),
        ("PUT", "/notascope/widget"),
        ("PUT", "/~npmjs/notascope/widget"),
        ("DELETE", "/notascope/widget/-/widget-1.0.0.tgz/-rev/1"),
        ("DELETE", "/~npmjs/notascope/widget/-/widget-1.0.0.tgz/-rev/1"),
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().method(method).uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {path}");
    }
}

#[tokio::test]
async fn a_method_the_address_does_not_serve_is_method_not_allowed() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.auth.htpasswd.max_users = MaxUsers::Unlimited;
    let app = router(config);

    for (method, path) in [
        // `/{name}` reads and publishes.
        ("DELETE", "/widget"),
        // `/{name}/-rev/{rev}` updates and unpublishes.
        ("GET", "/widget/-rev/1"),
        ("GET", "/~npmjs/widget/-rev/1"),
        // Search is a read.
        ("DELETE", "/-/v1/search"),
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().method(method).uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{method} {path}");
    }
}

/// A percent-escape that decodes to invalid UTF-8 is a malformed request, not
/// a server fault: answering 500 would both misreport it and let a client fill
/// the error log by looping on bad URLs.
#[tokio::test]
async fn a_prefix_that_is_not_valid_utf8_is_not_found() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.auth.htpasswd.max_users = MaxUsers::Unlimited;
    let app = router(config);

    let response =
        app.oneshot(Request::get("/%ff/-/whoami").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// A percent-encoded `#` or `?` is decoded before the handler sees it, so a
/// name carrying one would be authorized and cached under itself while the
/// upstream URL it is interpolated into addresses the bare name before the
/// delimiter.
#[tokio::test]
async fn url_delimiters_in_a_package_name_are_rejected() {
    let mut upstream = mockito::Server::new_async().await;
    let bare = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"name":"foo","versions":{}}"#)
        .expect(0)
        .create_async()
        .await;
    let tmp = TempDir::new().unwrap();
    let config = config_for(&upstream.url(), tmp.path().to_path_buf());

    for path in ["/foo%23bar", "/foo%3Fbar", "/foo%25bar", "/foo%20bar"] {
        let app = router(config.clone());
        let response = app.oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
    }
    bare.assert_async().await;
}

#[tokio::test]
async fn a_single_npm_ecosystem_answers_at_the_root() {
    let mut upstream = mockito::Server::new_async().await;
    let bytes = b"npm-tarball-bytes";
    let _packument = mock_packument_for_tarball(&mut upstream, "npm", "10.0.0", bytes).await;
    let tarball = upstream
        .mock("GET", "/npm/-/npm-10.0.0.tgz")
        .with_status(200)
        .with_body(bytes)
        .expect_at_least(1)
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for(&upstream.url(), tmp.path().to_path_buf()));

    let doc = app.clone().oneshot(Request::get("/npm").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(doc.status(), StatusCode::OK);
    let doc = body_json(doc.into_body()).await;
    let advertised = doc["versions"]["10.0.0"]["dist"]["tarball"].as_str().unwrap().to_string();
    assert_eq!(advertised, "http://example.test/npm/-/npm-10.0.0.tgz");

    let fetched = app
        .oneshot(
            Request::get(advertised.trim_start_matches("http://example.test"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(fetched.status(), StatusCode::OK);
    assert_eq!(body_bytes(fetched.into_body()).await, bytes);
    tarball.assert_async().await;
}

#[tokio::test]
async fn same_named_registries_keep_ecosystem_access_and_defaults_separate() {
    let tmp = TempDir::new().unwrap();
    let config = registry_groups::grouped_config(tmp.path(), "alice");
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config, auth);
    for authenticated in [false, true] {
        let directory = read_registry_directory(&app, authenticated.then_some(&token)).await;
        for ecosystem in Ecosystem::all() {
            assert_grouped_ecosystem(
                &directory,
                ecosystem,
                ecosystem != Ecosystem::Npm || authenticated,
            );
        }
    }
    for path in [
        "/cargo/~internal/index/config.json",
        "/cargo/~main/index/config.json",
        "/cargo/index/config.json",
        "/pypi/~internal/simple/",
        "/pypi/~main/simple/",
        "/pypi/simple/",
        "/oci/~internal/v2/",
        "/oci/~main/v2/",
        "/v2/",
    ] {
        let response =
            app.clone().oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(
            response.status(),
            if path.ends_with("/v2/") { StatusCode::UNAUTHORIZED } else { StatusCode::OK },
            "{path}",
        );
    }
    for path in [
        "/npm/~internal/demo",
        "/npm/~cargo%2Finternal/demo",
        "/cargo/~npm%2Finternal/index/config.json",
    ] {
        let response =
            app.clone().oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}
