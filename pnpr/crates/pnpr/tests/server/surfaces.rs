use super::{
    Body, Config, HeaderValue, Ipv4Addr, MaxUsers, Request, ServiceExt, SocketAddr, SocketAddrV4,
    StatusCode, TempDir, Value, body_bytes, body_json, config_for, header, json, router,
};
use pnpr_fixtures::MockConfigPresets;

#[tokio::test]
async fn ping_endpoint_returns_json_empty_object() {
    let tmp = TempDir::new().unwrap();
    let config = config_for("http://upstream.invalid", tmp.path().to_path_buf());
    let app = router(config);

    let response = app
        .oneshot(
            Request::get("/-/ping")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = body_bytes(response.into_body()).await;
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body, json!({}));
}

#[tokio::test]
async fn pipeline_surface_records_lists_and_serves_runs_append_only() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://upstream.invalid", tmp.path().to_path_buf());
    config.features.registry.enabled = false;
    config.features.resolver.enabled = false;
    config.features.pipeline.enabled = true;
    for (org, reader, writer) in
        [("demo-abc123", "alice", "alice"), ("hidden", "bob", "bob"), ("read-only", "alice", "bob")]
    {
        config.features.artifacts.orgs.insert(
            org.to_string(),
            pnpr_config::StorageAccess {
                access: pnpr_policy::AccessList::from_tokens([reader]),
                publish: pnpr_policy::AccessList::from_tokens([writer]),
            },
        );
    }
    config.identity.auth.htpasswd.max_users = MaxUsers::Unlimited;
    let app = router(config);

    let handshake = app
        .clone()
        .oneshot(
            Request::get("/-/pnpr")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        body_json(handshake.into_body()).await,
        json!({
            "pnpr": {
                "versions": [],
                "artifacts": [],
                "pipeline": [0],
                "fixLockfile": [],
                "ecosystems": [],
                "publish": [],
                "protocols": {
                    "resolver": {
                        "version": 0,
                        "versions": [],
                        "features": [
                            "fixLockfile",
                            "verifyLockfile",
                            "projectTransforms",
                            "policyEcho",
                            "errorCodes",
                            "acceptedLockfileFormats"
                        ],
                        "ecosystems": []
                    },
                    "artifacts": {
                        "version": 0,
                        "versions": []
                    },
                    "compilerCache": {
                        "version": 0,
                        "versions": []
                    },
                    "pipeline": {
                        "version": 0,
                        "versions": [0]
                    },
                    "publish": {
                        "version": 0,
                        "versions": [],
                        "pnpmPublish": []
                    }
                },
                "features": {
                    "registries": true,
                    "admin": false,
                    "scim": false,
                    "signIn": false
                }
            }
        }),
    );

    // Reads and writes both authenticate; the viewer page is static HTML
    // with no data of its own and does not.
    let anonymous = app
        .clone()
        .oneshot(
            Request::get("/-/pnpr/v0/pipeline/runs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    let viewer = app
        .clone()
        .oneshot(
            Request::get("/-/pnpr/v0/pipeline")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(viewer.status(), StatusCode::OK);

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
    let token = body_json(logged_in.into_body()).await["token"]
        .as_str()
        .unwrap()
        .to_string();

    let run = json!({
        "org": "demo-abc123",
        "workspace": "app",
        "runId": "100-default",
        "summary": { "pipeline": "default", "tasks": {} },
        "events": [{ "event": "taskStarted", "task": "packages/a#build" }],
    });
    let publish = |body: serde_json::Value| {
        let app = app.clone();
        let token = token.clone();
        async move {
            app.oneshot(
                Request::put("/-/pnpr/v0/pipeline/runs")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap()
        }
    };
    assert_eq!(publish(run.clone()).await.status(), StatusCode::CREATED);

    // Append-only: the same run identity is refused, not overwritten.
    let replay = publish(run.clone()).await;
    assert_eq!(replay.status(), StatusCode::BAD_REQUEST);
    assert!(String::from_utf8_lossy(&body_bytes(replay.into_body()).await).contains("append-only"));

    // A path-shaped organization never reaches a path join.
    let hostile = publish(json!({
        "org": "../escape",
        "workspace": "app",
        "runId": "100-default",
        "summary": {},
    }))
    .await;
    assert_eq!(hostile.status(), StatusCode::NOT_FOUND);

    for (org, expected) in [
        ("hidden", StatusCode::NOT_FOUND),
        ("unknown", StatusCode::NOT_FOUND),
        ("read-only", StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            publish(json!({"org": org, "workspace": "app", "runId": "100-default", "summary": {}}))
                .await
                .status(),
            expected,
        );
    }
    for path in
        ["/-/pnpr/v0/pipeline/runs?org=hidden", "/-/pnpr/v0/pipeline/runs/hidden/app/100-default"]
    {
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
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    let unfiltered = app
        .clone()
        .oneshot(
            Request::get("/-/pnpr/v0/pipeline/runs")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        body_json(unfiltered.into_body()).await["runs"]
            .as_array()
            .unwrap()
            .len(),
        1,
    );

    let listed = app
        .clone()
        .oneshot(
            Request::get("/-/pnpr/v0/pipeline/runs?org=demo-abc123&workspace=app")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let listed = body_json(listed.into_body()).await;
    assert_eq!(listed["runs"][0]["runId"], "100-default");
    assert_eq!(listed["runs"][0]["summary"]["pipeline"], "default");
    assert!(listed["runs"][0].get("events").is_none());

    let fetched = app
        .clone()
        .oneshot(
            Request::get("/-/pnpr/v0/pipeline/runs/demo-abc123/app/100-default")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(fetched.status(), StatusCode::OK);
    let fetched = body_json(fetched.into_body()).await;
    assert_eq!(fetched["events"][0]["event"], "taskStarted");

    let missing = app
        .oneshot(
            Request::get("/-/pnpr/v0/pipeline/runs/demo-abc123/app/999-missing")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cors_allows_only_configured_origins_and_handles_preflight() {
    let tmp = TempDir::new().unwrap();
    let mut config = Config::static_serve(
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
        tmp.path().to_path_buf(),
    );
    config.http.cors = pnpr::CorsConfig::from_allowed_origins(["https://npmx.example"]).unwrap();
    let app = router(config);

    let allowed = app
        .clone()
        .oneshot(
            Request::get("/-/ping")
                .header(header::ORIGIN, "https://npmx.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        allowed.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&HeaderValue::from_static("https://npmx.example")),
    );
    let vary = allowed
        .headers()
        .get(header::VARY)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        vary.split(',')
            .any(|header| header.trim().eq_ignore_ascii_case("origin")),
    );

    let missing = app
        .clone()
        .oneshot(
            Request::get("/missing")
                .header(header::ORIGIN, "https://npmx.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        missing.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&HeaderValue::from_static("https://npmx.example")),
    );

    let denied = app
        .clone()
        .oneshot(
            Request::get("/-/ping")
                .header(header::ORIGIN, "https://other.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        denied
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none(),
    );

    let preflight = app
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/-/v1/search?text=tool")
                .header(header::ORIGIN, "https://npmx.example")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preflight.status(), StatusCode::OK);
    assert_eq!(
        preflight.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&HeaderValue::from_static("https://npmx.example")),
    );
    let allowed_headers = preflight
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        allowed_headers
            .split(',')
            .any(|header| header.trim().eq_ignore_ascii_case("authorization")),
    );
}

#[tokio::test]
async fn readiness_and_liveness_endpoints_report_status() {
    let tmp = TempDir::new().unwrap();
    let config = config_for("http://upstream.invalid", tmp.path().to_path_buf());
    let app = router(config);

    // Bare root endpoints
    let live = app
        .clone()
        .oneshot(
            Request::get("/-/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(live.status(), StatusCode::OK);
    let live_body: Value = serde_json::from_slice(&body_bytes(live.into_body()).await).unwrap();
    assert_eq!(live_body, json!({"status": "ok"}));

    let ready = app
        .clone()
        .oneshot(
            Request::get("/-/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ready.status(), StatusCode::OK);
    let ready_body: Value = serde_json::from_slice(&body_bytes(ready.into_body()).await).unwrap();
    assert_eq!(
        ready_body,
        json!({
            "status": "ok",
            "storage": "ok",
            "auth": "ok"
        }),
    );

    // Prefixed registry endpoints
    let prefixed_live = app
        .clone()
        .oneshot(
            Request::get("/~hosted/-/live")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(prefixed_live.status(), StatusCode::OK);

    let prefixed_ready = app
        .clone()
        .oneshot(
            Request::get("/~hosted/-/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(prefixed_ready.status(), StatusCode::OK);

    let prefixed_ping = app
        .oneshot(
            Request::get("/~hosted/-/ping")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(prefixed_ping.status(), StatusCode::OK);
    let ping_body: Value =
        serde_json::from_slice(&body_bytes(prefixed_ping.into_body()).await).unwrap();
    assert_eq!(ping_body, json!({}));
}

#[tokio::test]
async fn npm_ping_answers_at_the_npm_registry_url() {
    let tmp = TempDir::new().unwrap();
    let app = router(config_for("http://upstream.invalid", tmp.path().to_path_buf()));
    for path in ["/npm/-/ping", "/npm/~hosted/-/ping"] {
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
    }
}

#[tokio::test]
async fn readiness_probe_fails_when_storage_is_inaccessible() {
    let tmp = TempDir::new().unwrap();
    // Create a regular file so that a child path cannot be created as a directory (ENOTDIR)
    let blocking_file = tmp.path().join("blocking-file");
    std::fs::write(&blocking_file, b"block").unwrap();
    let invalid_storage = blocking_file.join("sub-directory");

    let mut config = config_for("http://upstream.invalid", tmp.path().to_path_buf());
    config.storage.hosted_dir = invalid_storage;
    let app = router(config);

    // /-/ready returns 503 Service Unavailable when storage check fails
    let ready = app
        .clone()
        .oneshot(
            Request::get("/-/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ready.status(), StatusCode::SERVICE_UNAVAILABLE);
    let ready_body: Value = serde_json::from_slice(&body_bytes(ready.into_body()).await).unwrap();
    assert_eq!(ready_body["status"], "unhealthy");
    assert_eq!(ready_body["storage"], "unavailable");
    assert_eq!(ready_body["auth"], "ok");

    // /-/ping also returns 503 Service Unavailable when readiness fails
    let ping = app
        .oneshot(
            Request::get("/-/ping")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ping.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn readiness_probe_fails_when_auth_is_inaccessible() {
    let tmp = tempfile::tempdir().unwrap();
    let tokens_path = tmp.path().join("tokens.db");

    let mut config = config_for("http://upstream.invalid", tmp.path().to_path_buf());
    config.identity.auth.tokens.file = Some(tokens_path.clone());
    let auth = pnpr_auth::AuthState::load(&config.identity.auth, &config.identity.backend)
        .await
        .unwrap();
    let app = pnpr::router_with_auth(config, auth);

    // Remove the tokens database file so token store readiness fails
    std::fs::remove_file(&tokens_path).unwrap();

    // /-/ready returns 503 Service Unavailable when auth check fails
    let ready = app
        .clone()
        .oneshot(
            Request::get("/-/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ready.status(), StatusCode::SERVICE_UNAVAILABLE);
    let ready_body: Value = serde_json::from_slice(&body_bytes(ready.into_body()).await).unwrap();
    assert_eq!(ready_body["status"], "unhealthy");
    assert_eq!(ready_body["storage"], "ok");
    assert_eq!(ready_body["auth"], "unavailable");

    // /-/ping also returns 503 Service Unavailable when auth readiness fails
    let ping = app
        .oneshot(
            Request::get("/-/ping")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ping.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[cfg(unix)]
#[tokio::test]
async fn sigterm_signal_handler_installs_cleanly() {
    let mut signal =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
    // The signal stream exists and is ready to receive
    let poll_now = futures_util::future::poll_fn(|cx| signal.poll_recv(cx));
    tokio::pin!(poll_now);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), poll_now)
            .await
            .is_err(),
    );
}
