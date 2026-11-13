//! Integration tests for package ownership, the maintainer model,
//! $owners policy token, npm owner mutations, and package tombstones.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pnpr::Config;
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
        "storage: {}\nauth:\n  users:\n    maxUsers: 100\n{admin_line}registries:\n  \
         local:\n    type: hosted\n    namespace: \"\"\n    access: $all\n    packages:\n{nested}\n{catch_all}  \
         main:\n    type: router\n    sources: [local]\n\
         defaultRegistry: main\n",
        storage.display(),
    );
    let path = dir.path().join("config.yaml");
    std::fs::write(&path, yaml).unwrap();
    let config =
        pnpr_fixtures::load_seeded_config(&path, listen(), Some("http://example.test".to_string()));
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
    let accounts: Vec<_> = admins
        .iter()
        .map(|name| (*name, "secret123"))
        .collect();
    let auth = crate::tokens::seeded_auth(&config, &accounts);
    pnpr::router_with_auth(config, auth)
}

#[tokio::test]
async fn get_couchdb_user_endpoint() {
    let (_dir, config) = config_with_packages(
        "packages:\n  '**':\n    access: $all\n    publish: $authenticated\n",
        &["admin_alice", "unprovisioned_admin"],
    );
    let app = app_router(config, &["admin_alice"]).await;
    let (app, token) = add_user_and_get_token(app, "normal_bob", "secret123").await;

    // Check unprovisioned admin user returns NOT_FOUND
    let response = app
        .clone()
        .oneshot(
            Request::get("/-/user/org.couchdb.user:unprovisioned_admin")
                .header("authorization", format!("Bearer {token}"))
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
                .header("authorization", format!("Bearer {token}"))
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

    // Check normal user in htpasswd auth backend (legacy and bare user paths)
    for path in ["/-/user/org.couchdb.user:normal_bob", "/-/user/normal_bob"] {
        let response = app
            .clone()
            .oneshot(
                Request::get(path)
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "path {path}");
        let doc = body_json(response.into_body()).await;
        assert_eq!(doc["name"], "normal_bob");
        assert_eq!(doc["_id"], "org.couchdb.user:normal_bob");
    }

    // Check non-existent user
    let response = app
        .clone()
        .oneshot(
            Request::get("/-/user/org.couchdb.user:nobody")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let err = body_json(response.into_body()).await;
    assert_eq!(err["error"], "User not found");
    assert_eq!(err["error"], err["reason"]);
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
async fn publish_defaults_to_owners_for_an_entry_without_a_publish_list() {
    let (_dir, config) = config_with_packages("packages:\n  'default-lib': {}\n", &[]);
    let app = app_router(config, &[]).await;
    let (app, alice_token) = add_user_and_get_token(app, "alice", "secret123").await;
    let (app, bob_token) = add_user_and_get_token(app, "bob", "secret123").await;

    let doc = publish_doc("default-lib", "1.0.0", b"tarball-1");
    let response = app
        .clone()
        .oneshot(authed_put("/npm/default-lib", &alice_token, doc))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let doc = publish_doc("default-lib", "1.0.1", b"tarball-2");
    let response = app
        .clone()
        .oneshot(authed_put("/npm/default-lib", &bob_token, doc))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = app
        .clone()
        .oneshot(authed_put(
            "/npm/-/package/default-lib/dist-tags/next",
            &bob_token,
            json!("1.0.0"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let doc = publish_doc("default-lib", "1.0.1", b"tarball-2");
    let response = app
        .clone()
        .oneshot(authed_put("/npm/default-lib", &alice_token, doc))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn organization_listing_reports_write_for_maintainers_under_the_default_rule() {
    let (_dir, config) = config_with_packages("packages:\n  '@acme/*': {}\n", &[]);
    let app = app_router(config, &[]).await;
    let (app, alice_token) = add_user_and_get_token(app, "alice", "secret123").await;
    let (app, bob_token) = add_user_and_get_token(app, "bob", "secret123").await;
    for (pkg, token, tarball) in [
        ("@acme%2flib", &alice_token, &b"tarball-1"[..]),
        ("@acme%2ftool", &bob_token, &b"tarball-2"[..]),
    ] {
        let name = pkg.replace("%2f", "/");
        let doc = publish_doc(&name, "1.0.0", tarball);
        let response = app
            .clone()
            .oneshot(authed_put(&format!("/npm/{pkg}"), token, doc))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    for (token, expected) in [
        (&alice_token, json!({ "@acme/lib": "write", "@acme/tool": "read" })),
        (&bob_token, json!({ "@acme/lib": "read", "@acme/tool": "write" })),
    ] {
        let response = app
            .clone()
            .oneshot(authed_get("/npm/-/org/acme/package", token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_json(response.into_body()).await, expected);
    }

    let add_bob = json!({
        "_id": "@acme/lib",
        "_rev": "1-abc",
        "maintainers": [{ "name": "alice" }, { "name": "bob" }],
    });
    let response = app
        .clone()
        .oneshot(authed_put("/npm/@acme%2flib/-rev/1-abc", &alice_token, add_bob))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let response = app
        .clone()
        .oneshot(authed_get("/npm/-/org/acme/package", &bob_token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response.into_body()).await,
        json!({ "@acme/lib": "write", "@acme/tool": "write" }),
    );
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

    let (app, _) = add_user_and_get_token(app, "-", "secret123").await;
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
async fn a_packument_rewrite_whose_removal_record_fails_leaves_the_packument_unchanged() {
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

    let connection =
        rusqlite::Connection::open(dir.path().join("storage/.metadata/metadata.sqlite")).unwrap();
    connection.execute_batch("CREATE TRIGGER refuse_removal BEFORE INSERT ON removed_packages BEGIN SELECT RAISE(ABORT, 'injected removal failure'); END;").unwrap();

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

    // The transaction rolled back the package rewrite.
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

#[tokio::test]
async fn staged_publish_approval_rejects_malformed_body_without_claiming_stage() {
    let (_dir, config) = config_with_packages(
        "packages:\n  'staged-bad-body':\n    access: $all\n    publish: $authenticated\n",
        &[],
    );
    let app = app_router(config, &[]).await;
    let (app, alice_token) = add_user_and_get_token(app, "alice", "secret123").await;

    let doc = publish_doc("staged-bad-body", "1.0.0", b"v1");
    let res = app
        .clone()
        .oneshot(authed_post("/npm/-/stage/package/staged-bad-body", &alice_token, doc))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let stage_id = body_json(res.into_body()).await["stageId"]
        .as_str()
        .unwrap()
        .to_string();
    let approve_path = format!("/npm/-/stage/{stage_id}/approve");

    for bad_body in ["{not-json", "[]"] {
        let req = Request::builder()
            .method("POST")
            .uri(&approve_path)
            .header("Authorization", format!("Bearer {alice_token}"))
            .header("content-type", "application/json")
            .body(Body::from(bad_body))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{bad_body}");
        let err = body_json(res.into_body()).await;
        let message = err["error"].as_str().unwrap();
        assert!(message.starts_with("Bad request: invalid request body:"), "{message}");
    }

    let req = Request::builder()
        .method("POST")
        .uri(&approve_path)
        .header("Authorization", format!("Bearer {alice_token}"))
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn mutations_reauthorize_after_concurrent_package_update() {
    let primary_dir = TempDir::new().unwrap();
    let (primary, address, _server) =
        pnpr_libsql::testing::serve_primary(primary_dir.path().join("primary.db")).await;
    let (_dir, mut config) = config_with_packages(
        "packages:\n  'race-pkg':\n    access: $all\n    publish: $owners\n    unpublish: $owners\n",
        &[],
    );
    config.has_configured_secret = true;
    config.storage.metadata = pnpr_config::MetadataBackend::Libsql(pnpr_config::LibsqlSettings {
        url: format!("http://{address}"),
        auth_token: None,
        replica_path: None,
        sync_interval_secs: None,
        max_connections: None,
    });
    // The libSQL database holds no graph: serve the one seeded into SQLite.
    config.graph_source = pnpr_config::GraphSource::Supplied;
    let app = app_router(config, &[]).await;
    let (app, alice_token) = add_user_and_get_token(app, "alice", "secret123").await;
    let (app, bob_token) = add_user_and_get_token(app, "bob", "secret123").await;

    for (version, tarball) in [("1.0.0", b"v1".as_slice()), ("1.1.0", b"v2".as_slice())] {
        let doc = publish_doc("race-pkg", version, tarball);
        let res = app
            .clone()
            .oneshot(authed_put("/npm/race-pkg", &alice_token, doc))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
    }

    let restore_both_owners = || {
        let app = app.clone();
        let bob_token = bob_token.clone();
        async move {
            let body = json!({
                "_id": "race-pkg",
                "_rev": "1-abc",
                "maintainers": [{ "name": "alice" }, { "name": "bob" }],
            });
            let res = app
                .oneshot(authed_put("/npm/race-pkg/-rev/1-abc", &bob_token, body))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::CREATED);
        }
    };
    let set_initial = json!({
        "_id": "race-pkg",
        "_rev": "1-abc",
        "maintainers": [{ "name": "alice" }, { "name": "bob" }],
    });
    assert_eq!(
        app.clone()
            .oneshot(authed_put("/npm/race-pkg/-rev/1-abc", &alice_token, set_initial))
            .await
            .unwrap()
            .status(),
        StatusCode::CREATED,
    );

    let packument = body_json(
        app.clone()
            .oneshot(authed_get("/npm/race-pkg", &alice_token))
            .await
            .unwrap()
            .into_body(),
    )
    .await;
    let both_maintainers = packument["maintainers"].clone();
    let bob_only = json!([both_maintainers
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "bob")
        .unwrap()
        .clone()]);

    // When a concurrent commit advances the package generation without
    // removing Alice, her dist-tag update retries and succeeds.
    let status = run_with_concurrent_state_commit(
        &primary,
        &both_maintainers,
        app.clone(),
        authed_put("/npm/-/package/race-pkg/dist-tags/next", &alice_token, json!("1.0.0")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    restore_both_owners().await;
    let mut trimmed_packument = body_json(
        app.clone()
            .oneshot(authed_get("/npm/race-pkg", &alice_token))
            .await
            .unwrap()
            .into_body(),
    )
    .await;
    let rev = trimmed_packument["_rev"]
        .as_str()
        .unwrap()
        .to_string();
    trimmed_packument["versions"]
        .as_object_mut()
        .unwrap()
        .remove("1.1.0");
    trimmed_packument["dist-tags"]["latest"] = json!("1.0.0");
    let requests = [
        authed_put(&format!("/npm/race-pkg/-rev/{rev}"), &alice_token, trimmed_packument),
        authed_put("/npm/-/package/race-pkg/dist-tags/canary", &alice_token, json!("1.1.0")),
        authed_delete("/npm/-/package/race-pkg/dist-tags/next", &alice_token),
        authed_delete("/npm/race-pkg/-/race-pkg-1.1.0.tgz/-rev/1", &alice_token),
        authed_delete("/npm/race-pkg/-rev/1", &alice_token),
        authed_put(
            "/npm/race-pkg/-rev/1",
            &alice_token,
            json!({ "_id": "race-pkg", "_rev": "1", "maintainers": [{ "name": "alice" }] }),
        ),
    ];
    for request in requests {
        let uri = request.uri().to_string();
        let method = request.method().clone();
        let status =
            run_with_concurrent_state_commit(&primary, &bob_only, app.clone(), request).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}");
        restore_both_owners().await;
    }

    let final_doc = body_json(
        app.clone()
            .oneshot(authed_get("/npm/race-pkg", &bob_token))
            .await
            .unwrap()
            .into_body(),
    )
    .await;
    assert!(final_doc["versions"]["1.0.0"].is_object());
    assert!(final_doc["versions"]["1.1.0"].is_object());
    assert_eq!(final_doc["dist-tags"]["next"], "1.0.0");
    assert!(final_doc["dist-tags"].get("canary").is_none());
    assert_eq!(final_doc["maintainers"], both_maintainers);

    // When a concurrent replica removes the package before `remove_package_if_current`
    // commits, `PackageRemoval::NotFound` re-authorizes against the tombstone and
    // completes without exhausting the conflict retry loop.
    primary.round_trips.pause_next_write();
    let unpublish_task = tokio::spawn({
        let app = app.clone();
        let request = authed_delete("/npm/race-pkg/-rev/1", &alice_token);
        async move {
            app.oneshot(request).await
                .unwrap()
                .status()
        }
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        primary.round_trips.write_paused.notified(),
    )
    .await
    .expect("unpublish reaches its write transaction after pre-commit read");
    let maintainer_user_ids = serde_json::to_string(
        &both_maintainers
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["userId"].as_str().unwrap())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    primary
        .conn
        .execute(
            "INSERT INTO removed_packages (namespace, ecosystem, name, maintainers, unpublished_at, unpublished_by)
             VALUES ('', 'npm', 'race-pkg', ?1, 1000, 'bob')",
            [maintainer_user_ids],
        )
        .await
        .unwrap();
    primary.conn
        .execute(
            "DELETE FROM packages WHERE namespace = '' AND ecosystem = 'npm' AND name = 'race-pkg'",
            (),
        )
        .await
        .unwrap();
    primary.round_trips.write_resume.notify_one();
    assert_eq!(unpublish_task.await.unwrap(), StatusCode::CREATED);
}

async fn run_with_concurrent_state_commit(
    primary: &pnpr_libsql::testing::Primary,
    maintainers: &Value,
    app: axum::Router,
    request: Request<Body>,
) -> StatusCode {
    primary.round_trips.take();
    primary.round_trips.pause_next_write();
    let task = tokio::spawn(async move {
        app.oneshot(request).await
            .unwrap()
            .status()
    });
    // Wait until the handler has read and authorized the pre-commit document
    // and paused at its conditional write (`BEGIN IMMEDIATE`), then commit the
    // competing update before releasing the write so it conflicts and retries.
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        primary.round_trips.write_paused.notified(),
    )
    .await
    .expect("handler reaches its write transaction after pre-commit read");
    let mut rows = primary
        .conn
        .query(
            "SELECT state FROM packages WHERE namespace = '' AND ecosystem = 'npm' AND name = 'race-pkg'",
            (),
        )
        .await
        .unwrap();
    let state: Vec<u8> = rows
        .next()
        .await
        .unwrap()
        .unwrap()
        .get(0)
        .unwrap();
    drop(rows);
    let mut parsed: Value = serde_json::from_slice(&state).unwrap();
    parsed["maintainers"] = maintainers.clone();
    let updated_state = serde_json::to_vec(&parsed).unwrap();
    primary.conn
        .execute("UPDATE namespaces SET generation = generation + 1 WHERE namespace = ''", ())
        .await
        .unwrap();
    primary.conn
        .execute(
            "UPDATE packages
             SET generation = (SELECT generation FROM namespaces WHERE namespace = ''),
                 state = ?1
             WHERE namespace = '' AND ecosystem = 'npm' AND name = 'race-pkg'",
            [updated_state],
        )
        .await
        .unwrap();
    primary.round_trips.write_resume.notify_one();

    let status = task.await.unwrap();
    let reads = primary.round_trips
        .take()
        .iter()
        .flatten()
        .filter(|sql| sql.starts_with("SELECT generation, state_version, state FROM packages"))
        .count();
    assert!(reads >= 2, "expected initial read and retry read, got {reads}");
    status
}
