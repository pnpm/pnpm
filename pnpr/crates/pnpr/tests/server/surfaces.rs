use super::{
    Body, Config, HeaderValue, Ipv4Addr, MaxUsers, Request, ServiceExt, SocketAddr, SocketAddrV4,
    StatusCode, TempDir, Value, body_bytes, body_json, config_for, header, json, router,
};

#[tokio::test]
async fn ping_endpoint_returns_json_empty_object() {
    let tmp = TempDir::new().unwrap();
    let config = config_for("http://upstream.invalid", tmp.path().to_path_buf());
    let app = router(config);

    let response = app.oneshot(Request::get("/-/ping").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body_bytes = body_bytes(response.into_body()).await;
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(body, json!({}));
}

#[tokio::test]
async fn pipeline_surface_records_lists_and_serves_runs_append_only() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://upstream.invalid", tmp.path().to_path_buf());
    config.registry.enabled = false;
    config.resolver.enabled = false;
    config.pipeline.enabled = true;
    for (workspace, reader, writer) in
        [("demo-abc123", "alice", "alice"), ("hidden", "bob", "bob"), ("read-only", "alice", "bob")]
    {
        config.pipeline.workspaces.insert(
            workspace.to_string(),
            pnpr_config::StorageAccess {
                access: pnpr_policy::AccessList::from_tokens([reader]),
                publish: pnpr_policy::AccessList::from_tokens([writer]),
            },
        );
    }
    config.auth.htpasswd.max_users = MaxUsers::Unlimited;
    let app = router(config);

    let handshake =
        app.clone().oneshot(Request::get("/-/pnpr").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(
        body_json(handshake.into_body()).await,
        json!({ "pnpr": { "versions": [], "artifacts": [], "pipeline": [0], "fixLockfile": [], "ecosystems": [], "publish": [] } }),
    );

    // Reads and writes both authenticate; the viewer page is static HTML
    // with no data of its own and does not.
    let anonymous = app
        .clone()
        .oneshot(Request::get("/-/pnpr/v0/pipeline/runs").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    let viewer = app
        .clone()
        .oneshot(Request::get("/-/pnpr/v0/pipeline").body(Body::empty()).unwrap())
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
    let token = body_json(logged_in.into_body()).await["token"].as_str().unwrap().to_string();

    let run = json!({
        "workspace": "demo-abc123",
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

    // A path-shaped workspace never reaches a path join.
    let hostile = publish(json!({
        "workspace": "../escape",
        "runId": "100-default",
        "summary": {},
    }))
    .await;
    assert_eq!(hostile.status(), StatusCode::NOT_FOUND);

    for (workspace, expected) in [
        ("hidden", StatusCode::NOT_FOUND),
        ("unknown", StatusCode::NOT_FOUND),
        ("read-only", StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            publish(json!({"workspace": workspace, "runId": "100-default", "summary": {}}))
                .await
                .status(),
            expected,
        );
    }
    for path in
        ["/-/pnpr/v0/pipeline/runs?workspace=hidden", "/-/pnpr/v0/pipeline/runs/hidden/100-default"]
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
    assert_eq!(body_json(unfiltered.into_body()).await["runs"].as_array().unwrap().len(), 1);

    let listed = app
        .clone()
        .oneshot(
            Request::get("/-/pnpr/v0/pipeline/runs?workspace=demo-abc123")
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
            Request::get("/-/pnpr/v0/pipeline/runs/demo-abc123/100-default")
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
            Request::get("/-/pnpr/v0/pipeline/runs/demo-abc123/999-missing")
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
    config.cors = pnpr::CorsConfig::from_allowed_origins(["https://npmx.example"]).unwrap();
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
    let vary = allowed.headers().get(header::VARY).unwrap().to_str().unwrap();
    assert!(vary.split(',').any(|header| header.trim().eq_ignore_ascii_case("origin")));

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
    assert!(denied.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());

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
    let allowed_headers =
        preflight.headers().get(header::ACCESS_CONTROL_ALLOW_HEADERS).unwrap().to_str().unwrap();
    assert!(
        allowed_headers
            .split(',')
            .any(|header| header.trim().eq_ignore_ascii_case("authorization")),
    );
}
