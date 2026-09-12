use super::{
    AuthState, Body, Bytes, Duration, HeaderValue, Infallible, MaxUsers, Ordering, PublicRoute,
    Request, ServiceExt, StatusCode, TempDir, body_bytes, body_json, config_for,
    drain_resolve_response, git_resolve_request, header, json, router, router_with_auth,
    spawn_git_probe, stream, verify_lockfile_request,
};

#[tokio::test]
async fn anonymous_resolve_cannot_trigger_git_egress() {
    let (repo_url, request_count) = spawn_git_probe().await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for("http://127.0.0.1:1", tmp.path().to_path_buf()));
    let response = app.oneshot(git_resolve_request(&repo_url, None)).await.unwrap();
    let (status, body) = drain_resolve_response(response).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(String::from_utf8_lossy(&body).contains("Authentication required"));
    assert_eq!(request_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn default_registration_cannot_mint_a_resolver_credential() {
    let (repo_url, request_count) = spawn_git_probe().await;
    let tmp = TempDir::new().unwrap();
    let app = router(config_for("http://127.0.0.1:1", tmp.path().to_path_buf()));
    let registration = json!({
        "_id": "org.couchdb.user:outsider",
        "name": "outsider",
        "password": "secret",
        "email": "outsider@example.test",
        "type": "user",
        "roles": [],
    });
    let response = app
        .clone()
        .oneshot(
            Request::put("/-/user/org.couchdb.user:outsider")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&registration).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(!String::from_utf8_lossy(&body_bytes(response.into_body()).await).contains("token"));

    let response = app.oneshot(git_resolve_request(&repo_url, None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(request_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn anonymous_resolve_is_rejected_before_the_body_is_collected() {
    let tmp = TempDir::new().unwrap();
    let app = router(config_for("http://127.0.0.1:1", tmp.path().to_path_buf()));
    let body = Body::from_stream(stream::pending::<Result<Bytes, Infallible>>());
    let request = Request::post("/-/pnpr/v0/resolve")
        .header("content-type", "application/json")
        .header("content-length", "1000000")
        .body(body)
        .unwrap();

    let response = tokio::time::timeout(Duration::from_millis(250), app.oneshot(request))
        .await
        .expect("authentication must finish without waiting for the request body")
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn resolve_rejects_duplicate_authorization_headers() {
    let (repo_url, request_count) = spawn_git_probe().await;
    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config_for("http://127.0.0.1:1", tmp.path().to_path_buf()), auth);
    let mut request = git_resolve_request(&repo_url, Some(&format!("Bearer {token}")));
    request
        .headers_mut()
        .append(header::AUTHORIZATION, HeaderValue::from_static("Bearer invalid-second-value"));

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let mut reversed = git_resolve_request(&repo_url, None);
    reversed
        .headers_mut()
        .append(header::AUTHORIZATION, HeaderValue::from_static("Bearer invalid-first-value"));
    reversed
        .headers_mut()
        .append(header::AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {token}")).unwrap());
    let response = app.clone().oneshot(reversed).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let mut non_text = git_resolve_request(&repo_url, None);
    non_text.headers_mut().insert(header::AUTHORIZATION, HeaderValue::from_bytes(&[0xff]).unwrap());
    let response = app.oneshot(non_text).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(request_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn anonymous_verify_lockfile_cannot_trigger_registry_egress() {
    let (registry_url, request_count) = spawn_git_probe().await;

    let tmp = TempDir::new().unwrap();
    let app = router(config_for("http://127.0.0.1:1", tmp.path().to_path_buf()));
    let response = app.oneshot(verify_lockfile_request(&registry_url, None)).await.unwrap();
    let (status, body) = drain_resolve_response(response).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(String::from_utf8_lossy(&body).contains("Authentication required"));
    assert_eq!(request_count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn anonymous_verify_lockfile_is_rejected_before_the_body_is_collected() {
    let tmp = TempDir::new().unwrap();
    let app = router(config_for("http://127.0.0.1:1", tmp.path().to_path_buf()));
    let body = Body::from_stream(stream::pending::<Result<Bytes, Infallible>>());
    let request = Request::post("/-/pnpr/v0/verify-lockfile")
        .header("content-type", "application/json")
        .header("content-length", "1000000")
        .body(body)
        .unwrap();

    let response = tokio::time::timeout(Duration::from_millis(250), app.oneshot(request))
        .await
        .expect("authentication must finish without waiting for the request body")
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn authenticated_resolve_preserves_git_dependencies() {
    let (repo_url, request_count) = spawn_git_probe().await;

    let tmp = TempDir::new().unwrap();
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    // A git dependency's host must be on the fetch allowlist for the resolver
    // to reach it; an off-allowlist URL dependency is rejected at the request
    // boundary before any fetch.
    config
        .route_policy
        .public
        .push(PublicRoute { registry: Some(repo_url.clone()), package: None });
    let app = router_with_auth(config, auth);
    let response = app
        .oneshot(git_resolve_request(&repo_url, Some(&format!("Bearer {token}"))))
        .await
        .unwrap();
    assert_eq!(
        response.headers().get("pnpr-project-transforms").and_then(|value| value.to_str().ok()),
        Some("1"),
    );
    let (status, body) = drain_resolve_response(response).await;

    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&body).contains(r#""type":"error""#));
    assert!(request_count.load(Ordering::SeqCst) >= 1);
}

/// Mirrors the YAML rule at the programmatic layer: an upstream reachable
/// without an `access:` gate may not send custom headers — any header can
/// carry a credential, and an ungated `/~<name>/` would let every caller
/// spend it.
#[tokio::test]
async fn building_the_server_rejects_an_ungated_credentialed_upstream() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    let upstream = config.upstreams.get_mut("npmjs").expect("default `npmjs` upstream");
    upstream.headers.insert("x-api-key", "secret".parse().unwrap());
    let err =
        pnpr::try_router(config).expect_err("an ungated credentialed upstream must fail startup");
    assert!(err.to_string().contains("npmjs"), "unexpected error: {err}");
}

#[tokio::test]
async fn resolver_only_serves_resolver_endpoints_and_refuses_registry_routes() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://upstream.invalid", tmp.path().to_path_buf());
    config.registry.enabled = false;
    config.auth.htpasswd.max_users = MaxUsers::Unlimited;
    let app = router(config);

    // The resolver surface stays reachable. `/-/ping` and the capability
    // handshake answer 200; `/-/pnpr/v0/verify-lockfile` is mounted and gated,
    // so an anonymous request is a 401 rather than a 404 (route absent) — that
    // distinction is the point of the assertion.
    let ping =
        app.clone().oneshot(Request::get("/-/ping").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(ping.status(), StatusCode::OK);

    let handshake =
        app.clone().oneshot(Request::get("/-/pnpr").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(handshake.status(), StatusCode::OK);
    assert_eq!(
        body_json(handshake.into_body()).await,
        json!({
            "pnpr": {
                "versions": [0],
                "artifacts": [],
                "pipeline": [],
                "fixLockfile": [0],
                "ecosystems": ["npm", "cargo", "pypi"],
                "publish": [],
            }
        }),
    );

    let verify = app
        .clone()
        .oneshot(Request::post("/-/pnpr/v0/verify-lockfile").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(verify.status(), StatusCode::UNAUTHORIZED);

    // Every npm-registry route is gone, not merely hidden: a packument
    // read, a publish, and a batch publish all 404 without any upstream
    // call (the route itself is absent).
    let packument =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(packument.status(), StatusCode::NOT_FOUND);

    let publish =
        app.clone().oneshot(Request::put("/foo").body(Body::from("{}")).unwrap()).await.unwrap();
    assert_eq!(publish.status(), StatusCode::NOT_FOUND);

    let batch_publish = app
        .clone()
        .oneshot(Request::put("/-/pnpm/v1/publish").body(Body::from("{}")).unwrap())
        .await
        .unwrap();
    assert_eq!(batch_publish.status(), StatusCode::NOT_FOUND);

    // The account endpoints ride every tier: a resolver-only tier mints and
    // manages the tokens its own resolver surface demands, so `pnpm login
    // --registry https://<resolver-host>/` works without a registry-serving
    // replica that shares the auth backend.
    let registration = json!({
        "_id": "org.couchdb.user:alice",
        "name": "alice",
        "password": "secret",
        "email": "alice@example.test",
        "type": "user",
        "roles": [],
    });
    let logged_in = app
        .clone()
        .oneshot(
            Request::put("/-/user/org.couchdb.user:alice")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&registration).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logged_in.status(), StatusCode::CREATED);
    let token = body_json(logged_in.into_body()).await["token"].as_str().unwrap().to_string();
    // The `/~<prefix>/`-addressed twins answer too — they must not depend on
    // the (absent) registry segment routes.
    for path in [
        "/-/whoami",
        "/-/npm/v1/user",
        "/-/npm/v1/tokens",
        "/~corp/-/whoami",
        "/~corp/-/npm/v1/user",
        "/~corp/-/npm/v1/tokens",
    ] {
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
    let prefixed_login = app
        .clone()
        .oneshot(
            Request::put("/~corp/-/user/org.couchdb.user:alice")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&registration).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(prefixed_login.status(), StatusCode::CREATED);

    // The minted token authenticates against the resolver surface itself.
    let verify = app
        .clone()
        .oneshot(
            Request::post("/-/pnpr/v0/verify-lockfile")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(verify.status(), StatusCode::UNAUTHORIZED);
    assert_ne!(verify.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn registry_only_serves_registry_and_refuses_resolver_endpoints() {
    let mut upstream = mockito::Server::new_async().await;
    let mock = upstream
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(json!({ "name": "foo", "versions": {} }).to_string())
        .create_async()
        .await;

    let tmp = TempDir::new().unwrap();
    let mut config = config_for(&upstream.url(), tmp.path().to_path_buf());
    config.resolver.enabled = false;
    let app = router(config);

    // The registry surface still works: ping and a proxied packument read.
    let ping =
        app.clone().oneshot(Request::get("/-/ping").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(ping.status(), StatusCode::OK);

    let packument =
        app.clone().oneshot(Request::get("/foo").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(packument.status(), StatusCode::OK);

    // The registry tier has a pnpr protocol of its own — the cross-ecosystem
    // publish transaction — so the handshake answers, and reports no resolver.
    let handshake =
        app.clone().oneshot(Request::get("/-/pnpr").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(handshake.status(), StatusCode::OK);
    assert_eq!(
        body_json(handshake.into_body()).await,
        json!({
            "pnpr": {
                "versions": [],
                "artifacts": [],
                "pipeline": [],
                "fixLockfile": [],
                "ecosystems": [],
                "publish": [0],
            }
        }),
    );

    // With the resolver disabled its two endpoints are not mounted, and no
    // other route claims them.
    let resolve = app
        .clone()
        .oneshot(Request::post("/-/pnpr/v0/resolve").body(Body::from("{}")).unwrap())
        .await
        .unwrap();
    assert_eq!(resolve.status(), StatusCode::NOT_FOUND);

    let verify = app
        .clone()
        .oneshot(Request::post("/-/pnpr/v0/verify-lockfile").body(Body::from("{}")).unwrap())
        .await
        .unwrap();
    assert_eq!(verify.status(), StatusCode::NOT_FOUND);

    mock.assert_async().await;
}
