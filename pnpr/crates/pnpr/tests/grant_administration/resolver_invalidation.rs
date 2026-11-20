use super::{GRANTS, app_router, listen, login, public_url, put_grant, read, run_grants, send};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::Config;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

async fn post_ndjson(
    app: &Router,
    path: &str,
    token: Option<&str>,
    body: &Value,
) -> (StatusCode, String) {
    let mut request = Request::post(path).header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(
            request
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

fn done_lockfile(ndjson: &str) -> Value {
    let last = ndjson
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .expect("terminal frame");
    let frame: Value = serde_json::from_str(last).expect("valid JSON frame");
    assert_eq!(frame["type"], "done", "{ndjson}");
    frame["lockfile"].clone()
}

#[tokio::test]
async fn revoking_public_read_grant_invalidates_resolver_and_verdict_caches_immediately() {
    let dir = TempDir::new().unwrap();
    let storage = dir.path().join("storage");
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&storage).unwrap();
    std::fs::create_dir_all(&cache).unwrap();
    let yaml = format!(
        "\
storage:
  path: {}
cache:
  path: {}
secret: 0123456789abcdef0123456789abcdef
auth:
  admins: [root]
  registryAdmins: [root]
  users:
    maxUsers: 100
registries:
  local:
    type: hosted
    namespace: \"\"
    access: [alice]
defaultRegistry: local
",
        storage.display(),
        cache.display(),
    );
    let config_file = dir.path().join("pnpr.yaml");
    let config =
        crate::managed_registries::managed_config(&config_file, &yaml, listen(), public_url())
            .await;
    let app = app_router(config).await;

    let root = login(&app, "root").await;
    let alice = login(&app, "alice").await;
    let bob = login(&app, "bob").await;

    let open_grant = json!({
        "registries": ["local"],
        "subjects": {
            "$all": ["read"],
            "alice": ["read", "publish"],
        },
    });
    assert_eq!(put_grant(&app, &root, "local-access", open_grant.clone()).await.0, StatusCode::OK);

    let publish_doc = crate::npm::publish_doc("@acme/internal", "1.0.0", b"internal-code");
    assert_eq!(
        send(&app, "PUT", "/npm/@acme%2finternal", Some(&alice), Some(publish_doc))
            .await
            .0,
        StatusCode::CREATED,
    );

    let registry = "http://example.test/npm/";
    let range_req = json!({
        "registry": registry,
        "dependencies": { "@acme/internal": "^1.0.0" },
    });
    let exact_req = json!({
        "registry": registry,
        "dependencies": { "@acme/internal": "1.0.0" },
    });

    let (status, range_body) =
        post_ndjson(&app, "/-/pnpr/v0/resolve", Some(&bob), &range_req).await;
    assert_eq!(status, StatusCode::OK, "{range_body}");
    let lockfile = done_lockfile(&range_body);

    let (status, exact_body) =
        post_ndjson(&app, "/-/pnpr/v0/resolve", Some(&bob), &exact_req).await;
    assert_eq!(status, StatusCode::OK, "{exact_body}");
    let _ = done_lockfile(&exact_body);

    let verify_req = json!({
        "registry": registry,
        "dependencies": { "@acme/internal": "^1.0.0" },
        "lockfile": lockfile,
        "frozenLockfile": true,
        "trustPolicy": "no-downgrade",
    });
    let (status, verify_body) =
        post_ndjson(&app, "/-/pnpr/v0/verify-lockfile", Some(&bob), &verify_req).await;
    assert_eq!(status, StatusCode::OK, "{verify_body}");
    assert!(verify_body.contains(r#""type":"done""#), "{verify_body}");

    let alice_only = json!({
        "registries": ["local"],
        "subjects": {
            "alice": ["read", "publish"],
        },
    });
    assert_eq!(put_grant(&app, &root, "local-access", alice_only).await.0, StatusCode::OK);
    assert_eq!(read(&app, "/npm/@acme%2finternal", &bob).await, StatusCode::NOT_FOUND);

    for req in [&range_req, &exact_req] {
        let (bob_status, bob_body) = post_ndjson(&app, "/-/pnpr/v0/resolve", Some(&bob), req).await;
        assert_eq!(bob_status, StatusCode::OK);
        assert!(bob_body.contains(r#""type":"error""#), "bob must not resolve: {bob_body}");
        assert_eq!(
            post_ndjson(&app, "/-/pnpr/v0/resolve", None, req).await.0,
            StatusCode::UNAUTHORIZED,
        );
    }
    assert_eq!(
        post_ndjson(&app, "/-/pnpr/v0/verify-lockfile", Some(&bob), &verify_req)
            .await
            .0,
        StatusCode::BAD_GATEWAY,
    );
    assert_eq!(
        post_ndjson(&app, "/-/pnpr/v0/resolve", Some(&bob), &verify_req).await.0,
        StatusCode::BAD_GATEWAY,
    );
    assert_eq!(
        post_ndjson(&app, "/-/pnpr/v0/verify-lockfile", None, &verify_req).await.0,
        StatusCode::UNAUTHORIZED,
    );

    for path in ["/-/pnpr/v0/resolve", "/-/pnpr/v0/verify-lockfile"] {
        let (alice_status, alice_body) = post_ndjson(&app, path, Some(&alice), &verify_req).await;
        assert_eq!(alice_status, StatusCode::OK, "{path}: {alice_body}");
        assert!(alice_body.contains(r#""type":"done""#), "{path}: {alice_body}");
    }

    let public_extra = json!({
        "registries": ["local"],
        "subjects": { "$all": ["read"] },
    });
    assert_eq!(put_grant(&app, &root, "temp-public", public_extra).await.0, StatusCode::OK);
    let (_, warmed) = post_ndjson(&app, "/-/pnpr/v0/resolve", Some(&bob), &range_req).await;
    let _ = done_lockfile(&warmed);
    assert_eq!(
        post_ndjson(&app, "/-/pnpr/v0/verify-lockfile", Some(&bob), &verify_req)
            .await
            .0,
        StatusCode::OK,
    );

    assert_eq!(
        send(&app, "DELETE", &format!("{GRANTS}/temp-public"), Some(&root), None)
            .await
            .0,
        StatusCode::NO_CONTENT,
    );
    let (_, bob_after_delete) =
        post_ndjson(&app, "/-/pnpr/v0/resolve", Some(&bob), &range_req).await;
    assert!(bob_after_delete.contains(r#""type":"error""#), "{bob_after_delete}");
    assert_eq!(
        post_ndjson(&app, "/-/pnpr/v0/verify-lockfile", Some(&bob), &verify_req)
            .await
            .0,
        StatusCode::BAD_GATEWAY,
    );

    assert_eq!(put_grant(&app, &root, "local-access", open_grant).await.0, StatusCode::OK);
    assert_eq!(
        post_ndjson(&app, "/-/pnpr/v0/verify-lockfile", Some(&bob), &verify_req)
            .await
            .0,
        StatusCode::OK,
    );
    let grants_file = dir.path().join("restrictive-grants.yaml");
    std::fs::write(
        &grants_file,
        "grants:\n  local-access:\n    registries: [local]\n    subjects:\n      alice: [read, publish]\n",
    )
    .unwrap();
    let offline_cfg = Config::from_yaml(&config_file, listen(), public_url()).unwrap();
    run_grants(
        &offline_cfg,
        pnpr::GrantsSubcommand::Import { file: grants_file, dry_run: false, check: false },
    )
    .await
    .unwrap();

    let restarted =
        app_router(Config::from_yaml(&config_file, listen(), public_url()).unwrap()).await;
    let bob_restarted = login(&restarted, "bob").await;
    let alice_restarted = login(&restarted, "alice").await;
    assert_eq!(
        post_ndjson(&restarted, "/-/pnpr/v0/verify-lockfile", Some(&bob_restarted), &verify_req)
            .await
            .0,
        StatusCode::BAD_GATEWAY,
    );
    let (_, bob_restarted_resolve) =
        post_ndjson(&restarted, "/-/pnpr/v0/resolve", Some(&bob_restarted), &range_req).await;
    assert!(bob_restarted_resolve.contains(r#""type":"error""#), "{bob_restarted_resolve}");
    assert_eq!(
        post_ndjson(&restarted, "/-/pnpr/v0/verify-lockfile", Some(&alice_restarted), &verify_req)
            .await
            .0,
        StatusCode::OK,
    );
}
