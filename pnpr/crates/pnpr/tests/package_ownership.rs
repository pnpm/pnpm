//! Integration tests for package ownership, the maintainer model,
//! $owners policy token, npm owner mutations, and package tombstones.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::Config;
use pnpr_storage::{
    TOMBSTONES_NAMESPACE,
    layout::{encode_segment, namespace_segments},
};
use serde_json::{Value, json};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use tempfile::TempDir;
use tower::ServiceExt;

use crate::npm::publish_doc;

fn listen() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873))
}

fn config_with_packages(packages_block: &str, admins: &[&str]) -> (TempDir, Config) {
    let dir = TempDir::new().unwrap();
    let storage = dir.path().join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    let nested: String = packages_block
        .strip_prefix("packages:\n")
        .expect("callers pass a packages: block")
        .lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let admin_line = if admins.is_empty() {
        String::new()
    } else {
        let items: Vec<String> = admins
            .iter()
            .map(|admin| format!(r#""{admin}""#))
            .collect();
        format!("  admins: [{}]\n", items.join(", "))
    };
    let catch_all = if packages_block.contains("'**'") { "" } else { "      '**': {}\n" };
    let yaml = format!(
        "storage: {}\nauth:\n  htpasswd:\n    max_users: 100\n{admin_line}registries:\n  \
         local:\n    type: hosted\n    org: \"\"\n    access: $all\n    packages:\n{nested}\n{catch_all}  \
         main:\n    type: router\n    sources: [local]\n\
         defaultRegistry: main\n",
        storage.display(),
    );
    let path = dir.path().join("config.yaml");
    std::fs::write(&path, yaml).unwrap();
    let config =
        Config::from_yaml(&path, listen(), Some("http://example.test".to_string())).unwrap();
    (dir, config)
}

async fn body_bytes(body: Body) -> Vec<u8> {
    to_bytes(body, usize::MAX).await.expect("read body").to_vec()
}

async fn body_json(body: Body) -> Value {
    serde_json::from_slice(&body_bytes(body).await).expect("body parses as JSON")
}

async fn add_user_and_get_token(
    app: axum::Router,
    username: &str,
    password: &str,
) -> (axum::Router, String) {
    let path = format!("/-/user/org.couchdb.user:{username}");
    let body = json!({
        "_id": format!("org.couchdb.user:{username}"),
        "name": username,
        "password": password,
        "email": format!("{username}@example.test"),
        "type": "user",
        "roles": [],
    });
    let request = Request::put(&path)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app
        .clone()
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let payload = body_json(response.into_body()).await;
    let token = payload["token"]
        .as_str()
        .expect("token in response")
        .to_string();
    (app, token)
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called with owned literals; by-value keeps call sites clean"
)]
fn authed_put(path: &str, token: &str, body: Value) -> Request<Body> {
    Request::put(path)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

fn authed_delete(path: &str, token: &str) -> Request<Body> {
    Request::delete(path)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called with owned literals; by-value keeps call sites clean"
)]
fn authed_post(path: &str, token: &str, body: Value) -> Request<Body> {
    Request::post(path)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

fn authed_get(path: &str, token: &str) -> Request<Body> {
    Request::get(path)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

async fn app_router(config: Config, admins: &[&str]) -> axum::Router {
    let auth = pnpr::AuthState::in_memory_with_max_users(config.identity.auth.htpasswd.max_users);
    for admin in admins {
        auth.users.create_user(admin, "secret123").await.unwrap();
    }
    pnpr::router_with_auth(config, auth)
}

#[tokio::test]
async fn get_couchdb_user_endpoint() {
    let (_dir, config) = config_with_packages(
        "packages:\n  '**':\n    access: $all\n    publish: $authenticated\n",
        &["admin_alice", "unprovisioned_admin"],
    );
    let app = app_router(config, &["admin_alice"]).await;
    let (app, _) = add_user_and_get_token(app, "normal_bob", "secret123").await;

    // Check unprovisioned admin user returns NOT_FOUND
    let response = app
        .clone()
        .oneshot(
            Request::get("/-/user/org.couchdb.user:unprovisioned_admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // Check provisioned admin user
    let response = app
        .clone()
        .oneshot(
            Request::get("/-/user/org.couchdb.user:admin_alice")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let doc = body_json(response.into_body()).await;
    assert_eq!(doc["name"], "admin_alice");
    assert_eq!(doc["_id"], "org.couchdb.user:admin_alice");
    assert_eq!(doc["roles"], json!([]));

    // Check normal user in htpasswd auth backend
    let response = app
        .clone()
        .oneshot(
            Request::get("/-/user/org.couchdb.user:normal_bob")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let doc = body_json(response.into_body()).await;
    assert_eq!(doc["name"], "normal_bob");
    assert_eq!(doc["_id"], "org.couchdb.user:normal_bob");

    // Check non-existent user
    let response = app
        .clone()
        .oneshot(
            Request::get("/-/user/org.couchdb.user:nobody")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let err = body_json(response.into_body()).await;
    assert_eq!(err["error"], "not_found");
}

#[tokio::test]
async fn first_publish_claims_package_in_maintainers() {
    let (_dir, config) = config_with_packages(
        "packages:\n  '**':\n    access: $all\n    publish: $authenticated\n",
        &[],
    );
    let app = app_router(config, &[]).await;
    let (app, alice_token) = add_user_and_get_token(app, "alice", "secret123").await;

    let doc = publish_doc("my-pkg", "1.0.0", b"tarball-v1");
    let response = app
        .clone()
        .oneshot(authed_put("/npm/my-pkg", &alice_token, doc))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let response = app
        .clone()
        .oneshot(
            Request::get("/npm/my-pkg")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let packument = body_json(response.into_body()).await;
    let maintainers = packument["maintainers"].as_array().expect("maintainers array");
    assert_eq!(maintainers.len(), 1);
    assert_eq!(maintainers[0]["name"], "alice");
}

#[tokio::test]
async fn owners_policy_restricts_subsequent_publish() {
    let (_dir, config) = config_with_packages(
        "packages:\n  'owned-lib':\n    access: $all\n    publish: $owners\n    unpublish: $owners\n",
        &["admin_user"],
    );
    let app = app_router(config, &["admin_user"]).await;
    let (app, alice_token) = add_user_and_get_token(app, "alice", "secret123").await;
    let (app, bob_token) = add_user_and_get_token(app, "bob", "secret123").await;
    let (app, admin_token) = add_user_and_get_token(app, "admin_user", "secret123").await;

    // Initial publish by Alice claims the package
    let doc = publish_doc("owned-lib", "1.0.0", b"tarball-1");
    let response = app
        .clone()
        .oneshot(authed_put("/npm/owned-lib", &alice_token, doc))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // Bob (not an owner, not an admin) is rejected from publishing version 1.0.1
    let doc = publish_doc("owned-lib", "1.0.1", b"tarball-2");
    let response = app
        .clone()
        .oneshot(authed_put("/npm/owned-lib", &bob_token, doc))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Alice (the owner) can publish version 1.0.1
    let doc = publish_doc("owned-lib", "1.0.1", b"tarball-2");
    let response = app
        .clone()
        .oneshot(authed_put("/npm/owned-lib", &alice_token, doc))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // Admin can also publish version 1.0.2
    let doc = publish_doc("owned-lib", "1.0.2", b"tarball-3");
    let response = app
        .clone()
        .oneshot(authed_put("/npm/owned-lib", &admin_token, doc))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn npm_owner_maintainer_mutation_flow() {
    let (_dir, config) = config_with_packages(
        "packages:\n  'team-pkg':\n    access: $all\n    publish: $owners\n    unpublish: $owners\n",
        &["admin_z"],
    );
    let app = app_router(config, &["admin_z"]).await;
    let (app, alice_token) = add_user_and_get_token(app, "alice", "secret123").await;
    let (app, bob_token) = add_user_and_get_token(app, "bob", "secret123").await;
    let (app, admin_token) = add_user_and_get_token(app, "admin_z", "secret123").await;

    // Alice claims team-pkg
    let doc = publish_doc("team-pkg", "1.0.0", b"content-1");
    let res = app
        .clone()
        .oneshot(authed_put("/npm/team-pkg", &alice_token, doc))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // Non-owner Bob tries to mutate maintainers -> 403
    let mutation = json!({
        "_id": "team-pkg",
        "_rev": "1-abc",
        "maintainers": [{ "name": "bob" }]
    });
    let res = app
        .clone()
        .oneshot(authed_put("/npm/team-pkg/-rev/1-abc", &bob_token, mutation))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // Owner Alice tries to empty maintainers -> 400 EmptyMaintainers
    let empty_mutation = json!({
        "_id": "team-pkg",
        "_rev": "1-abc",
        "maintainers": []
    });
    let res = app
        .clone()
        .oneshot(authed_put("/npm/team-pkg/-rev/1-abc", &alice_token, empty_mutation))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    // Alice adds Bob as maintainer
    let add_bob_mutation = json!({
        "_id": "team-pkg",
        "_rev": "1-abc",
        "maintainers": [
            { "name": "alice" },
            { "name": "bob" }
        ]
    });
    let res = app
        .clone()
        .oneshot(authed_put("/npm/team-pkg/-rev/1-abc", &alice_token, add_bob_mutation))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // Now Bob is an owner and can publish 1.0.1
    let doc = publish_doc("team-pkg", "1.0.1", b"content-2");
    let res = app
        .clone()
        .oneshot(authed_put("/npm/team-pkg", &bob_token, doc))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // Admin removes Alice, leaving only Bob
    let admin_mutation = json!({
        "_id": "team-pkg",
        "_rev": "2-def",
        "maintainers": [{ "name": "bob" }]
    });
    let res = app
        .clone()
        .oneshot(authed_put("/npm/team-pkg/-rev/2-def", &admin_token, admin_mutation))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // Alice is no longer a maintainer -> 403 on publish
    let doc = publish_doc("team-pkg", "1.0.2", b"content-3");
    let res = app
        .clone()
        .oneshot(authed_put("/npm/team-pkg", &alice_token, doc))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn anonymous_caller_cannot_change_maintainers() {
    let (_dir, config) = config_with_packages(
        "packages:\n  'open-pkg':\n    access: $all\n    publish: $all\n",
        &[],
    );
    let app = app_router(config, &[]).await;
    let (app, alice_token) = add_user_and_get_token(app, "alice", "secret123").await;

    let doc = publish_doc("open-pkg", "1.0.0", b"content-1");
    let res = app
        .clone()
        .oneshot(authed_put("/npm/open-pkg", &alice_token, doc))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // `-` is the name the audit log gives a caller without a username.
    let dash_maintainer = json!({ "_id": "open-pkg", "maintainers": [{ "name": "-" }] });
    let res = app
        .clone()
        .oneshot(authed_put("/npm/open-pkg/-rev/1-abc", &alice_token, dash_maintainer))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let takeover = json!({ "_id": "open-pkg", "maintainers": [{ "name": "mallory" }] });
    let res = app
        .clone()
        .oneshot(
            Request::put("/npm/open-pkg/-rev/2-def")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&takeover).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn package_tombstones_prevent_squatting_and_version_republishing() {
    let (_dir, config) = config_with_packages(
        "packages:\n  'tomb-pkg':\n    access: $all\n    publish: $owners\n    unpublish: $owners\n",
        &[],
    );
    let app = app_router(config, &[]).await;
    let (app, alice_token) = add_user_and_get_token(app, "alice", "secret123").await;
    let (app, charlie_token) = add_user_and_get_token(app, "charlie", "secret123").await;

    // Alice publishes 1.0.0 and 1.1.0
    let doc1 = publish_doc("tomb-pkg", "1.0.0", b"v1");
    let res = app
        .clone()
        .oneshot(authed_put("/npm/tomb-pkg", &alice_token, doc1))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let doc2 = publish_doc("tomb-pkg", "1.1.0", b"v2");
    let res = app
        .clone()
        .oneshot(authed_put("/npm/tomb-pkg", &alice_token, doc2))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // Alice unpublishes the entire package
    let res = app
        .clone()
        .oneshot(authed_delete("/npm/tomb-pkg/-rev/1", &alice_token))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // Charlie (not former maintainer) tries to publish tomb-pkg -> rejected 403 PackageTombstoned
    let doc_squat = publish_doc("tomb-pkg", "1.0.0", b"malicious");
    let res = app
        .clone()
        .oneshot(authed_put("/npm/tomb-pkg", &charlie_token, doc_squat))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    let doc_squat_new = publish_doc("tomb-pkg", "2.0.0", b"malicious2");
    let res = app
        .clone()
        .oneshot(authed_put("/npm/tomb-pkg", &charlie_token, doc_squat_new))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // Alice (former maintainer) tries to republish previously unpublished version 1.0.0 -> rejected 403
    let doc_repub = publish_doc("tomb-pkg", "1.0.0", b"v1-again");
    let res = app
        .clone()
        .oneshot(authed_put("/npm/tomb-pkg", &alice_token, doc_repub))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // Alice can republish the package with a new version 2.0.0
    let doc_new = publish_doc("tomb-pkg", "2.0.0", b"v2-new");
    let res = app
        .clone()
        .oneshot(authed_put("/npm/tomb-pkg", &alice_token, doc_new))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // Alice partial-unpublishes 2.0.0 by deleting tarball
    let res = app
        .clone()
        .oneshot(authed_delete("/npm/tomb-pkg/-/tomb-pkg-2.0.0.tgz/-rev/1", &alice_token))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // Now 2.0.0 is unpublished and cannot be republished
    let doc_repub_2 = publish_doc("tomb-pkg", "2.0.0", b"v2-again");
    let res = app
        .clone()
        .oneshot(authed_put("/npm/tomb-pkg", &alice_token, doc_repub_2))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // But 2.0.1 can be published
    let doc_201 = publish_doc("tomb-pkg", "2.0.1", b"v2.0.1");
    let res = app
        .clone()
        .oneshot(authed_put("/npm/tomb-pkg", &alice_token, doc_201))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn scoped_package_ownership_and_unpublish() {
    let (_dir, config) = config_with_packages(
        "packages:\n  '@my-org/*':\n    access: $all\n    publish: $owners\n    unpublish: $owners\n",
        &[],
    );
    let app = app_router(config, &[]).await;
    let (app, alice_token) = add_user_and_get_token(app, "alice", "secret123").await;
    let (app, bob_token) = add_user_and_get_token(app, "bob", "secret123").await;
    let (app, charlie_token) = add_user_and_get_token(app, "charlie", "secret123").await;

    // Alice publishes @my-org/core@1.0.0
    let doc = publish_doc("@my-org/core", "1.0.0", b"scoped-1");
    let res = app
        .clone()
        .oneshot(authed_put("/npm/@my-org/core", &alice_token, doc))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // Bob cannot update maintainers
    let mutation = json!({
        "_id": "@my-org/core",
        "_rev": "1-abc",
        "maintainers": [{ "name": "bob" }]
    });
    let res = app
        .clone()
        .oneshot(authed_put("/npm/@my-org/core/-rev/1-abc", &bob_token, mutation))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // Alice adds Bob as maintainer
    let add_bob = json!({
        "_id": "@my-org/core",
        "_rev": "1-abc",
        "maintainers": [
            { "name": "alice" },
            { "name": "bob" }
        ]
    });
    let res = app
        .clone()
        .oneshot(authed_put("/npm/@my-org/core/-rev/1-abc", &alice_token, add_bob))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // Bob unpublishes the scoped package
    let res = app
        .clone()
        .oneshot(authed_delete("/npm/@my-org/core/-rev/1", &bob_token))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // Charlie cannot squat @my-org/core -> 403 PackageTombstoned
    let squat = publish_doc("@my-org/core", "1.0.0", b"evil");
    let res = app
        .clone()
        .oneshot(authed_put("/npm/@my-org/core", &charlie_token, squat))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn cannot_unpublish_or_lock_nonexistent_package() {
    let (_dir, config) = config_with_packages(
        "packages:\n  '**':\n    access: $all\n    publish: $owners\n    unpublish: $owners\n",
        &["admin_alice"],
    );
    let app = app_router(config, &["admin_alice"]).await;
    let (app, eve_token) = add_user_and_get_token(app, "eve", "secret123").await;

    // Eve tries to DELETE a tarball for a nonexistent package
    let res = app
        .clone()
        .oneshot(authed_delete("/npm/unclaimed-pkg/-/unclaimed-pkg-1.0.0.tgz/-rev/1", &eve_token))
        .await
        .unwrap();
    assert!(res.status() == StatusCode::FORBIDDEN || res.status() == StatusCode::NOT_FOUND);

    // Eve can still claim the package normally by publishing it first
    let doc = publish_doc("unclaimed-pkg", "1.0.0", b"legit");
    let res = app
        .clone()
        .oneshot(authed_put("/npm/unclaimed-pkg", &eve_token, doc))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn staged_publish_under_owners_rule_can_be_approved_by_maintainer() {
    let (_dir, config) = config_with_packages(
        "packages:\n  'staged-owner-pkg':\n    access: $all\n    publish: $owners\n    unpublish: $owners\n",
        &[],
    );
    let app = app_router(config, &[]).await;
    let (app, alice_token) = add_user_and_get_token(app, "alice", "secret123").await;
    let (app, bob_token) = add_user_and_get_token(app, "bob", "secret123").await;

    // Alice publishes 1.0.0 directly -> Alice becomes maintainer
    let doc1 = publish_doc("staged-owner-pkg", "1.0.0", b"v1");
    let res = app
        .clone()
        .oneshot(authed_put("/npm/staged-owner-pkg", &alice_token, doc1))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // Alice stages 1.1.0
    let doc2 = publish_doc("staged-owner-pkg", "1.1.0", b"v2");
    let res = app
        .clone()
        .oneshot(authed_post("/npm/-/stage/package/staged-owner-pkg", &alice_token, doc2))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let body: Value = serde_json::from_slice(&body_bytes(res.into_body()).await).unwrap();
    let stage_id = body["stageId"].as_str().unwrap();

    // Bob (not maintainer) attempts to approve the staged publish -> 403 Forbidden
    let res = app
        .clone()
        .oneshot(authed_post(&format!("/npm/-/stage/{stage_id}/approve"), &bob_token, json!({})))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // Alice (maintainer) approves the staged publish -> 201 Created
    let res = app
        .clone()
        .oneshot(authed_post(&format!("/npm/-/stage/{stage_id}/approve"), &alice_token, json!({})))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    // Verify 1.1.0 is published
    let res = app
        .clone()
        .oneshot(authed_get("/npm/staged-owner-pkg", &alice_token))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let packument: Value = serde_json::from_slice(&body_bytes(res.into_body()).await).unwrap();
    assert!(packument["versions"]["1.1.0"].is_object());
}

#[tokio::test]
async fn packument_rewrite_failure_during_tombstone_write_restores_prior_packument() {
    let (dir, config) = config_with_packages(
        "packages:\n  'fail-pkg':\n    access: $all\n    publish: $owners\n    unpublish: $owners\n",
        &[],
    );
    let app = app_router(config, &[]).await;
    let (app, alice_token) = add_user_and_get_token(app, "alice", "secret123").await;

    // Alice publishes 1.0.0 and 1.1.0
    let doc1 = publish_doc("fail-pkg", "1.0.0", b"v1");
    let res = app
        .clone()
        .oneshot(authed_put("/npm/fail-pkg", &alice_token, doc1))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let doc2 = publish_doc("fail-pkg", "1.1.0", b"v2");
    let res = app
        .clone()
        .oneshot(authed_put("/npm/fail-pkg", &alice_token, doc2))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);

    let res = app
        .clone()
        .oneshot(authed_get("/npm/fail-pkg", &alice_token))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let mut packument: Value = serde_json::from_slice(&body_bytes(res.into_body()).await).unwrap();
    assert!(packument["versions"]["1.0.0"].is_object());
    assert!(packument["versions"]["1.1.0"].is_object());

    // Prepare rewrite removing 1.1.0
    packument["versions"]
        .as_object_mut()
        .unwrap()
        .remove("1.1.0");
    if let Some(dist_tags) = packument.get_mut("dist-tags").and_then(Value::as_object_mut) {
        dist_tags.insert("latest".to_string(), json!("1.0.0"));
    }

    // Block tombstone recording by creating a directory where the tombstone file belongs
    let mut tombstone_file = dir.path().join("storage");
    tombstone_file.extend(namespace_segments(""));
    tombstone_file.push(TOMBSTONES_NAMESPACE);
    tombstone_file.push(format!("{}.json", encode_segment("fail-pkg")));
    std::fs::create_dir_all(&tombstone_file).unwrap();

    // Alice attempts to rewrite the packument to remove 1.1.0
    let res = app
        .clone()
        .oneshot(authed_put("/npm/fail-pkg/-rev/1", &alice_token, packument))
        .await
        .unwrap();
    assert!(
        res.status() == StatusCode::BAD_GATEWAY || res.status() == StatusCode::CONFLICT,
        "request must fail when tombstone recording fails (status: {})",
        res.status(),
    );

    // Verify rollback restored the original packument so 1.1.0 is still present
    let res = app
        .clone()
        .oneshot(authed_get("/npm/fail-pkg", &alice_token))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let restored: Value = serde_json::from_slice(&body_bytes(res.into_body()).await).unwrap();
    assert!(restored["versions"]["1.1.0"].is_object());
    assert!(restored["versions"]["1.0.0"].is_object());
}
