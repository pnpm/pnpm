//! Integration tests for the `-/stage` endpoints — the server half of
//! `pnpm stage`. Static-mode (no upstream) to keep the tests hermetic.

#[path = "common/npm.rs"]
mod npm;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use npm::{publish_doc, sha1_hex};
use pnpr::{Config, MaxUsers, router};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::PathBuf,
};
use tempfile::TempDir;
use tower::ServiceExt;

fn static_config(storage: PathBuf) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::static_serve(listen, storage);
    config.public_url = "http://example.test".to_string();
    config.auth.htpasswd.max_users = MaxUsers::Unlimited;
    config
}

async fn body_bytes(body: Body) -> Vec<u8> {
    to_bytes(body, usize::MAX).await.expect("read body").to_vec()
}

async fn body_json(body: Body) -> Value {
    serde_json::from_slice(&body_bytes(body).await).expect("body parses as JSON")
}

fn request(method: &str, path: &str, body: Body, token: Option<&str>) -> Request<Body> {
    let mut builder =
        Request::builder().method(method).uri(path).header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("Authorization", format!("Bearer {token}"));
    }
    builder.body(body).unwrap()
}

fn json_request(method: &str, path: &str, body: &Value, token: Option<&str>) -> Request<Body> {
    request(method, path, Body::from(serde_json::to_vec(body).unwrap()), token)
}

async fn add_user_and_get_token(app: axum::Router, username: &str, password: &str) -> String {
    let path = format!("/-/user/org.couchdb.user:{username}");
    let body = json!({
        "_id": format!("org.couchdb.user:{username}"),
        "name": username,
        "password": password,
        "email": "foo@bar.net",
        "type": "user",
        "roles": [],
    });
    let response = app.oneshot(json_request("PUT", &path, &body, None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let payload = body_json(response.into_body()).await;
    payload["token"].as_str().expect("token in response").to_string()
}

/// Stage `doc` and return the stage id the registry minted.
async fn stage_package(app: axum::Router, name: &str, doc: &Value, token: &str) -> String {
    let path = format!("/-/stage/package/{}", name.replace('/', "%2f"));
    let response = app.oneshot(json_request("POST", &path, doc, Some(token))).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let payload = body_json(response.into_body()).await;
    payload["stageId"].as_str().expect("stageId in response").to_string()
}

fn is_uuid(value: &str) -> bool {
    value.len() == 36
        && value.char_indices().all(|(index, char)| match index {
            8 | 13 | 18 | 23 => char == '-',
            _ => char.is_ascii_hexdigit(),
        })
}

#[tokio::test]
async fn staged_publish_is_held_back_until_approved() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;

    let tarball = b"staged-tarball";
    let doc = publish_doc("staged-pkg", "1.0.0", tarball);
    let stage_id = stage_package(app.clone(), "staged-pkg", &doc, &token).await;
    assert!(is_uuid(&stage_id), "stage id must be a UUID: {stage_id}");

    // Held back: the package is not installable and nothing is on disk
    // under its name.
    let read = app
        .clone()
        .oneshot(request("GET", "/staged-pkg", Body::empty(), Some(&token)))
        .await
        .unwrap();
    assert_eq!(read.status(), StatusCode::NOT_FOUND);
    assert!(!storage.join("staged-pkg").exists(), "no package dir before approval");

    // Listed, viewable, and its tarball downloadable.
    let list = app
        .clone()
        .oneshot(request("GET", "/-/stage?page=0&perPage=100", Body::empty(), Some(&token)))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let listed = body_json(list.into_body()).await;
    assert_eq!(listed["total"], 1);
    assert_eq!(listed["items"][0]["id"], Value::String(stage_id.clone()));
    assert_eq!(listed["items"][0]["packageName"], "staged-pkg");
    assert_eq!(listed["items"][0]["version"], "1.0.0");
    assert_eq!(listed["items"][0]["tag"], "latest");
    assert_eq!(listed["items"][0]["actor"], "alice");
    assert_eq!(listed["items"][0]["actorType"], "user");
    assert_eq!(listed["items"][0]["shasum"], Value::String(sha1_hex(tarball)));
    assert!(listed["items"][0].get("registry").is_none(), "routing state must not be served");

    let view = app
        .clone()
        .oneshot(request("GET", &format!("/-/stage/{stage_id}"), Body::empty(), Some(&token)))
        .await
        .unwrap();
    assert_eq!(view.status(), StatusCode::OK);
    let viewed = body_json(view.into_body()).await;
    assert_eq!(viewed["packageName"], "staged-pkg");

    let download = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/-/stage/{stage_id}/tarball"),
            Body::empty(),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(download.status(), StatusCode::OK);
    assert_eq!(body_bytes(download.into_body()).await, tarball);

    // Approve: the package becomes installable and the record disappears.
    let approve = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/-/stage/{stage_id}/approve"),
            Body::empty(),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(approve.status(), StatusCode::CREATED);

    let read = app
        .clone()
        .oneshot(request("GET", "/staged-pkg", Body::empty(), Some(&token)))
        .await
        .unwrap();
    assert_eq!(read.status(), StatusCode::OK);
    let packument = body_json(read.into_body()).await;
    assert_eq!(packument["versions"]["1.0.0"]["version"], "1.0.0");
    let on_disk = std::fs::read(storage.join("staged-pkg/staged-pkg-1.0.0.tgz"))
        .expect("tarball promoted on approval");
    assert_eq!(on_disk, tarball);

    let list =
        app.clone().oneshot(request("GET", "/-/stage", Body::empty(), Some(&token))).await.unwrap();
    let listed = body_json(list.into_body()).await;
    assert_eq!(listed["total"], 0, "an approved stage leaves no record behind");
    let view = app
        .oneshot(request("GET", &format!("/-/stage/{stage_id}"), Body::empty(), Some(&token)))
        .await
        .unwrap();
    assert_eq!(view.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn rejecting_a_staged_publish_deletes_it_without_publishing() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;

    let doc = publish_doc("rejected-pkg", "1.0.0", b"bytes");
    let stage_id = stage_package(app.clone(), "rejected-pkg", &doc, &token).await;

    let reject = app
        .clone()
        .oneshot(request("DELETE", &format!("/-/stage/{stage_id}"), Body::empty(), Some(&token)))
        .await
        .unwrap();
    assert_eq!(reject.status(), StatusCode::NO_CONTENT);

    let view = app
        .clone()
        .oneshot(request("GET", &format!("/-/stage/{stage_id}"), Body::empty(), Some(&token)))
        .await
        .unwrap();
    assert_eq!(view.status(), StatusCode::NOT_FOUND);
    let read =
        app.oneshot(request("GET", "/rejected-pkg", Body::empty(), Some(&token))).await.unwrap();
    assert_eq!(read.status(), StatusCode::NOT_FOUND);
    assert!(!storage.join("rejected-pkg").exists(), "a rejected stage publishes nothing");
}

#[tokio::test]
async fn staging_supports_scoped_packages() {
    let tmp = TempDir::new().unwrap();
    let storage = tmp.path().to_path_buf();
    let app = router(static_config(storage.clone()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;

    let tarball = b"scoped-bytes";
    let doc = publish_doc("@scope/staged", "2.0.0", tarball);
    let stage_id = stage_package(app.clone(), "@scope/staged", &doc, &token).await;

    let list = app
        .clone()
        .oneshot(request("GET", "/-/stage?package=%40scope%2Fstaged", Body::empty(), Some(&token)))
        .await
        .unwrap();
    let listed = body_json(list.into_body()).await;
    assert_eq!(listed["total"], 1);
    assert_eq!(listed["items"][0]["packageName"], "@scope/staged");

    let approve = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/-/stage/{stage_id}/approve"),
            Body::empty(),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(approve.status(), StatusCode::CREATED);
    let on_disk = std::fs::read(storage.join("@scope/staged/staged-2.0.0.tgz"))
        .expect("scoped tarball promoted on approval");
    assert_eq!(on_disk, tarball);
}

#[tokio::test]
async fn the_package_filter_narrows_the_listing() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;

    stage_package(app.clone(), "filter-a", &publish_doc("filter-a", "1.0.0", b"a"), &token).await;
    stage_package(app.clone(), "filter-b", &publish_doc("filter-b", "1.0.0", b"b"), &token).await;

    let list = app
        .clone()
        .oneshot(request("GET", "/-/stage?package=filter-a", Body::empty(), Some(&token)))
        .await
        .unwrap();
    let listed = body_json(list.into_body()).await;
    assert_eq!(listed["total"], 1);
    assert_eq!(listed["items"][0]["packageName"], "filter-a");

    let all = app.oneshot(request("GET", "/-/stage", Body::empty(), Some(&token))).await.unwrap();
    let listed = body_json(all.into_body()).await;
    assert_eq!(listed["total"], 2);
}

#[tokio::test]
async fn pagination_slices_the_listing() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;

    for index in 0..3 {
        let name = format!("paged-{index}");
        stage_package(app.clone(), &name, &publish_doc(&name, "1.0.0", b"x"), &token).await;
    }

    let page = app
        .clone()
        .oneshot(request("GET", "/-/stage?page=0&perPage=2", Body::empty(), Some(&token)))
        .await
        .unwrap();
    let first = body_json(page.into_body()).await;
    assert_eq!(first["total"], 3);
    assert_eq!(first["perPage"], 2);
    assert_eq!(first["items"].as_array().map(Vec::len), Some(2));

    let page = app
        .oneshot(request("GET", "/-/stage?page=1&perPage=2", Body::empty(), Some(&token)))
        .await
        .unwrap();
    let second = body_json(page.into_body()).await;
    assert_eq!(second["page"], 1);
    assert_eq!(second["items"].as_array().map(Vec::len), Some(1));
}

#[tokio::test]
async fn staging_requires_the_publish_right() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));

    let doc = publish_doc("anon-staged", "1.0.0", b"bytes");
    let response = app
        .oneshot(json_request("POST", "/-/stage/package/anon-staged", &doc, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn approving_requires_the_publish_right() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;

    let doc = publish_doc("guarded-pkg", "1.0.0", b"bytes");
    let stage_id = stage_package(app.clone(), "guarded-pkg", &doc, &token).await;

    let approve = app
        .clone()
        .oneshot(request("POST", &format!("/-/stage/{stage_id}/approve"), Body::empty(), None))
        .await
        .unwrap();
    assert_eq!(approve.status(), StatusCode::UNAUTHORIZED);
    let reject = app
        .clone()
        .oneshot(request("DELETE", &format!("/-/stage/{stage_id}"), Body::empty(), None))
        .await
        .unwrap();
    assert_eq!(reject.status(), StatusCode::UNAUTHORIZED);

    // An anonymous listing shows nothing rather than leaking the record.
    let list = app.oneshot(request("GET", "/-/stage", Body::empty(), None)).await.unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let listed = body_json(list.into_body()).await;
    assert_eq!(listed["total"], 0);
}

#[tokio::test]
async fn approving_a_version_published_in_the_meantime_conflicts() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;

    let doc = publish_doc("conflicted-pkg", "1.0.0", b"staged-bytes");
    let stage_id = stage_package(app.clone(), "conflicted-pkg", &doc, &token).await;

    // The same version lands through a direct publish before approval.
    let direct = publish_doc("conflicted-pkg", "1.0.0", b"direct-bytes");
    let response = app
        .clone()
        .oneshot(json_request("PUT", "/conflicted-pkg", &direct, Some(&token)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let approve = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/-/stage/{stage_id}/approve"),
            Body::empty(),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(approve.status(), StatusCode::CONFLICT);

    // The record survives a failed approval; it can still be rejected.
    let reject = app
        .oneshot(request("DELETE", &format!("/-/stage/{stage_id}"), Body::empty(), Some(&token)))
        .await
        .unwrap();
    assert_eq!(reject.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_bogus_stage_id_is_not_found_or_rejected() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;

    let unknown = "1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f";
    let view = app
        .clone()
        .oneshot(request("GET", &format!("/-/stage/{unknown}"), Body::empty(), Some(&token)))
        .await
        .unwrap();
    assert_eq!(view.status(), StatusCode::NOT_FOUND);

    // A path-traversal-shaped id is rejected before it can reach storage.
    let hostile = app
        .oneshot(request("GET", "/-/stage/%2e%2e%2fescape", Body::empty(), Some(&token)))
        .await
        .unwrap();
    assert_eq!(hostile.status(), StatusCode::BAD_REQUEST);
}

/// Approving a stage spends it: the transaction committed, so the record goes
/// even when it reports a package it could not put in the document. Leaving it
/// listed would offer an approval that cannot happen again.
#[tokio::test]
async fn an_approval_that_reports_a_conflict_still_consumes_the_stage() {
    use object_store::{ObjectStore, ObjectStoreExt, memory::InMemory, path::Path as ObjectPath};
    use pnpr::HostedStoreConfig;
    use std::sync::Arc;

    let tmp = TempDir::new().unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let mut config = static_config(tmp.path().to_path_buf());
    config.hosted_store =
        HostedStoreConfig::ObjectStore { store: Arc::clone(&store), prefix: String::new() };
    let app = router(config);
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;
    let doc = publish_doc("staged-pkg", "1.0.0", b"the losing tarball");
    let stage_id = stage_package(app.clone(), "staged-pkg", &doc, &token).await;
    // Another writer takes the tarball key while the stage waits for approval.
    store
        .put(
            &ObjectPath::from("staged-pkg/staged-pkg-1.0.0.tgz"),
            axum::body::Bytes::from_static(b"the winning tarball").into(),
        )
        .await
        .unwrap();

    let approve = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/-/stage/{stage_id}/approve"),
            Body::empty(),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(approve.status(), StatusCode::CONFLICT);

    let list = app
        .oneshot(request("GET", "/-/stage?page=0&perPage=100", Body::empty(), Some(&token)))
        .await
        .unwrap();
    assert_eq!(body_json(list.into_body()).await["total"], 0, "the approved stage is spent");
}

/// Rewrite the stored record of `stage_id` as if another replica had claimed
/// it for approval `age` ago. The record lives in the hosted store, which is
/// the one thing replicas of a stateless deployment share.
fn claim_stage_on_disk(storage: &std::path::Path, stage_id: &str, age: chrono::Duration) {
    let since = chrono::Utc::now() - age;
    write_claim_on_disk(
        storage,
        stage_id,
        &since.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    );
}

/// Stamp `approvingSince` verbatim, for the values a well-behaved replica
/// does not write.
fn write_claim_on_disk(storage: &std::path::Path, stage_id: &str, since: &str) {
    let path = storage.join(".staged").join(format!("{stage_id}.json"));
    let mut record: Value =
        serde_json::from_slice(&std::fs::read(&path).expect("staged record on disk")).unwrap();
    record["approvingSince"] = json!(since);
    std::fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
}

/// Replicas time their claims by their own clocks. A claim from a replica
/// running ahead of this one must hold: reading it as expired would hand the
/// stage to a second approval while the first is still publishing it.
#[tokio::test]
async fn a_claim_from_a_replica_whose_clock_runs_ahead_holds() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;
    let doc = publish_doc("staged-pkg", "1.0.0", b"the tarball");
    let stage_id = stage_package(app.clone(), "staged-pkg", &doc, &token).await;
    claim_stage_on_disk(tmp.path(), &stage_id, -chrono::Duration::minutes(2));

    let approve = app
        .oneshot(request(
            "POST",
            &format!("/-/stage/{stage_id}/approve"),
            Body::empty(),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(approve.status(), StatusCode::CONFLICT);
}

/// A claim pnpr cannot read must not be able to hold a stage forever.
#[tokio::test]
async fn a_claim_pnpr_cannot_read_does_not_hold_the_stage() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;
    let doc = publish_doc("staged-pkg", "1.0.0", b"the tarball");
    let stage_id = stage_package(app.clone(), "staged-pkg", &doc, &token).await;
    write_claim_on_disk(tmp.path(), &stage_id, "whenever");

    let approve = app
        .oneshot(request(
            "POST",
            &format!("/-/stage/{stage_id}/approve"),
            Body::empty(),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(approve.status(), StatusCode::CREATED);
}

/// A stage is approved once. While another replica's approval holds the
/// record, this one is refused rather than replaying the held publish a
/// second time.
#[tokio::test]
async fn an_approval_is_refused_while_another_holds_the_record() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;
    let doc = publish_doc("staged-pkg", "1.0.0", b"the tarball");
    let stage_id = stage_package(app.clone(), "staged-pkg", &doc, &token).await;
    claim_stage_on_disk(tmp.path(), &stage_id, chrono::Duration::seconds(3));

    let approve = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/-/stage/{stage_id}/approve"),
            Body::empty(),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(approve.status(), StatusCode::CONFLICT);
    let message = String::from_utf8(body_bytes(approve.into_body()).await).unwrap();
    assert!(message.contains("already being approved"), "unexpected message: {message}");

    let packument =
        app.clone().oneshot(request("GET", "/staged-pkg", Body::empty(), None)).await.unwrap();
    assert_eq!(packument.status(), StatusCode::NOT_FOUND, "the held publish stays held");

    let list = app
        .oneshot(request("GET", "/-/stage?page=0&perPage=100", Body::empty(), Some(&token)))
        .await
        .unwrap();
    assert_eq!(body_json(list.into_body()).await["total"], 1, "the stage is still approvable");
}

/// A replica that dies mid-approval leaves its claim behind. The claim is a
/// lease, so the stage becomes approvable again instead of being stranded
/// until someone rejects it.
#[tokio::test]
async fn a_claim_left_behind_by_a_dead_replica_expires() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;
    let doc = publish_doc("staged-pkg", "1.0.0", b"the tarball");
    let stage_id = stage_package(app.clone(), "staged-pkg", &doc, &token).await;
    claim_stage_on_disk(tmp.path(), &stage_id, chrono::Duration::hours(1));

    let approve = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/-/stage/{stage_id}/approve"),
            Body::empty(),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(approve.status(), StatusCode::CREATED);

    let packument = app.oneshot(request("GET", "/staged-pkg", Body::empty(), None)).await.unwrap();
    assert_eq!(packument.status(), StatusCode::OK);
}

/// An approval that fails leaves the stage exactly as it found it: the claim
/// is released, so the next attempt is not turned away as a second approval.
#[tokio::test]
async fn a_failed_approval_releases_its_claim() {
    let tmp = TempDir::new().unwrap();
    let app = router(static_config(tmp.path().to_path_buf()));
    let token = add_user_and_get_token(app.clone(), "alice", "secret").await;
    let doc = publish_doc("staged-pkg", "1.0.0", b"the tarball");
    let stage_id = stage_package(app.clone(), "staged-pkg", &doc, &token).await;
    // Publishing the staged version directly makes every approval of the
    // stage fail the same way, whatever its claim does.
    let published =
        app.clone().oneshot(json_request("PUT", "/staged-pkg", &doc, Some(&token))).await.unwrap();
    assert_eq!(published.status(), StatusCode::CREATED);

    for attempt in 0..2 {
        let approve = app
            .clone()
            .oneshot(request(
                "POST",
                &format!("/-/stage/{stage_id}/approve"),
                Body::empty(),
                Some(&token),
            ))
            .await
            .unwrap();
        assert_eq!(approve.status(), StatusCode::CONFLICT);
        let message = String::from_utf8(body_bytes(approve.into_body()).await).unwrap();
        assert!(
            message.contains("previously published version"),
            "attempt {attempt} must report the publish conflict, not a claim: {message}",
        );
    }
}
