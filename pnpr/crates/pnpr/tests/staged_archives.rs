//! Staging release archives by integrity ahead of the publish commit that
//! references them: `PUT` and `HEAD /-/pnpr/v0/artifacts/blob?integrity=...`
//! with no owner, and the publishes that reference what was staged.

use crate::npm::{npm_tgz, publish_doc, sri_sha512};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use pnpr::{Config, MaxUsers, router};
use pnpr_config::MetadataBackend;
use pnpr_fixtures::MockConfigPresets;
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::Path,
};
use tempfile::TempDir;
use tower::ServiceExt;

pub(crate) fn config(storage: &Path) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::static_serve(listen, storage.to_path_buf());
    config.http.public_url = "http://example.test".to_string();
    config.identity.auth.htpasswd.max_users = MaxUsers::Unlimited;
    config
}

pub(crate) struct Answer {
    pub(crate) status: StatusCode,
    pub(crate) length: Option<String>,
    pub(crate) text: String,
}

/// Send one blob request. `body` is sent with its length declared.
pub(crate) async fn blob(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&[u8]>,
) -> Answer {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(body) = body {
        request = request.header(header::CONTENT_LENGTH, body.len());
    }
    let body = body.map_or_else(Body::empty, |body| Body::from(body.to_vec()));
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let length = response
        .headers()
        .get(header::CONTENT_LENGTH)
        .map(|value| value.to_str().unwrap().to_string());
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    Answer { status, length, text: String::from_utf8_lossy(&bytes).into_owned() }
}

pub(crate) fn staging_path(integrity: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("integrity", integrity)
        .finish();
    format!("/-/pnpr/v0/artifacts/blob?{query}")
}

pub(crate) async fn token_of(app: &Router, user: &str) -> String {
    let body = json!({ "name": user, "password": "secret", "type": "user", "roles": [] });
    let response = app
        .clone()
        .oneshot(
            Request::put(format!("/-/user/org.couchdb.user:{user}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    body["token"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn an_archive_is_staged_once_and_only_for_its_uploader() {
    let tmp = TempDir::new().unwrap();
    let app = router(config(tmp.path()));
    let alice = token_of(&app, "alice").await;
    let bob = token_of(&app, "bob").await;
    let archive = npm_tgz("staged", "1.0.0", b"payload");
    let path = staging_path(&sri_sha512(&archive));

    let missing = blob(&app, "HEAD", &path, Some(&alice), None).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);

    let created = blob(&app, "PUT", &path, Some(&alice), Some(&archive)).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text);
    let again = blob(&app, "PUT", &path, Some(&alice), Some(&archive)).await;
    assert_eq!(again.status, StatusCode::OK, "uploading stored bytes again is a no-op");

    let stored = blob(&app, "HEAD", &path, Some(&alice), None).await;
    assert_eq!(stored.status, StatusCode::OK);
    assert_eq!(stored.length, Some(archive.len().to_string()));
    // Under the npm base too, where `pnpm publish --batch` addresses it.
    let npm_base = blob(&app, "HEAD", &format!("/npm{path}"), Some(&alice), None).await;
    assert_eq!(npm_base.status, StatusCode::OK);

    let other_principal = blob(&app, "HEAD", &path, Some(&bob), None).await;
    assert_eq!(other_principal.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bytes_that_do_not_match_their_integrity_are_refused() {
    let tmp = TempDir::new().unwrap();
    let app = router(config(tmp.path()));
    let alice = token_of(&app, "alice").await;
    let archive = npm_tgz("staged", "1.0.0", b"payload");
    let path = staging_path(&sri_sha512(b"other bytes"));

    let refused = blob(&app, "PUT", &path, Some(&alice), Some(&archive)).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert!(refused.text.contains("EINTEGRITY"), "{}", refused.text);
    let missing = blob(&app, "HEAD", &path, Some(&alice), None).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn staging_needs_a_caller_a_length_and_a_sha512_integrity() {
    let tmp = TempDir::new().unwrap();
    let app = router(config(tmp.path()));
    let alice = token_of(&app, "alice").await;
    let archive = npm_tgz("staged", "1.0.0", b"payload");
    let path = staging_path(&sri_sha512(&archive));

    let anonymous = blob(&app, "PUT", &path, None, Some(&archive)).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);

    let response = app
        .clone()
        .oneshot(
            Request::put(&path)
                .header(header::AUTHORIZATION, format!("Bearer {alice}"))
                .body(Body::from(archive.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST, "no Content-Length");

    let sha1 = staging_path("sha1-AAAAAAAAAAAAAAAAAAAAAAAAAAA=");
    let refused = blob(&app, "PUT", &sha1, Some(&alice), Some(&archive)).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_archive_over_the_publish_limit_is_refused_before_it_is_read() {
    let tmp = TempDir::new().unwrap();
    let app = router(config(tmp.path()));
    let alice = token_of(&app, "alice").await;
    let response = app
        .clone()
        .oneshot(
            Request::put(staging_path(&sri_sha512(b"large")))
                .header(header::AUTHORIZATION, format!("Bearer {alice}"))
                .header(header::CONTENT_LENGTH, 200 * 1024 * 1024)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("limit"));
}

/// [`publish_doc`] with its attachment naming the staged `tgz` instead of
/// carrying it.
fn staged_doc(name: &str, version: &str, tgz: &[u8]) -> Value {
    let mut doc = publish_doc(name, version, tgz);
    for attachment in doc["_attachments"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        *attachment = json!({ "integrity": sri_sha512(tgz), "length": tgz.len() });
    }
    doc
}

async fn send_json(app: &Router, method: &str, path: &str, token: &str, body: &Value) -> Answer {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(serde_json::to_vec(body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    Answer { status, length: None, text: String::from_utf8_lossy(&bytes).into_owned() }
}

async fn get(app: &Router, path: &str, token: &str) -> (StatusCode, Vec<u8>) {
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
    let status = response.status();
    (status, to_bytes(response.into_body(), usize::MAX).await.unwrap().to_vec())
}

async fn stage(app: &Router, tgz: &[u8], token: &str) {
    let staged = blob(app, "PUT", &staging_path(&sri_sha512(tgz)), Some(token), Some(tgz)).await;
    assert_eq!(staged.status, StatusCode::CREATED, "{}", staged.text);
}

fn sqlite_config(storage: &Path) -> Config {
    let mut config = config(storage);
    config.storage.metadata = MetadataBackend::Sqlite(storage.join("metadata.sqlite"));
    config
}

#[tokio::test]
async fn a_publish_takes_its_archive_from_the_publishers_staged_archives() {
    let tmp = TempDir::new().unwrap();
    let app = router(config(tmp.path()));
    let alice = token_of(&app, "alice").await;
    let tgz = npm_tgz("referenced", "1.0.0", b"payload");
    stage(&app, &tgz, &alice).await;

    let published =
        send_json(&app, "PUT", "/npm/referenced", &alice, &staged_doc("referenced", "1.0.0", &tgz))
            .await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.text);
    let (status, tarball) = get(&app, "/npm/referenced/-/referenced-1.0.0.tgz", &alice).await;
    assert_eq!((status, tarball), (StatusCode::OK, tgz.clone()));
    // Claimed, so it no longer counts against the publisher.
    let released = blob(&app, "HEAD", &staging_path(&sri_sha512(&tgz)), Some(&alice), None).await;
    assert_eq!(released.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn competing_publishes_each_take_their_own_staged_archive() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let alice = token_of(&app, "alice").await;
    let first = npm_tgz("first", "1.0.0", b"first");
    let second = npm_tgz("second", "1.0.0", b"second");
    stage(&app, &first, &alice).await;
    stage(&app, &second, &alice).await;

    let first_doc = staged_doc("first", "1.0.0", &first);
    let second_doc = json!({ "packages": [staged_doc("second", "1.0.0", &second)] });
    let single = send_json(&app, "PUT", "/npm/first", &alice, &first_doc);
    let batch = send_json(&app, "PUT", "/-/pnpr/v0/publish", &alice, &second_doc);
    let (single, batch) = tokio::join!(single, batch);
    assert_eq!(single.status, StatusCode::CREATED, "{}", single.text);
    assert_eq!(batch.status, StatusCode::CREATED, "{}", batch.text);

    for (name, tgz) in [("first", &first), ("second", &second)] {
        let path = format!("/npm/{name}/-/{name}-1.0.0.tgz");
        assert_eq!(get(&app, &path, &alice).await, (StatusCode::OK, tgz.clone()));
    }
}

#[tokio::test]
async fn a_batch_of_staged_archives_is_one_publication_and_a_retry_replays_it() {
    let tmp = TempDir::new().unwrap();
    let config = sqlite_config(tmp.path());
    let staging = config.storage.hosted_dir.join(".staged-archives");
    let app = router(config);
    let alice = token_of(&app, "alice").await;
    let core = npm_tgz("core", "2.0.0", b"core");
    let widgets = npm_tgz("widgets", "2.0.0", b"widgets");
    stage(&app, &core, &alice).await;
    stage(&app, &widgets, &alice).await;
    let batch = json!({
        "name": "v2.0.0",
        "packages": [staged_doc("core", "2.0.0", &core), staged_doc("widgets", "2.0.0", &widgets)],
    });

    let committed = send_json(&app, "PUT", "/-/pnpr/v0/publish", &alice, &batch).await;
    assert_eq!(committed.status, StatusCode::CREATED, "{}", committed.text);
    let (status, publication) = get(&app, "/-/publications/-/v2.0.0", &alice).await;
    assert_eq!(status, StatusCode::OK);
    let publication: Value = serde_json::from_slice(&publication).unwrap();
    assert_eq!(
        publication["members"]
            .as_array()
            .unwrap()
            .len(),
        2,
    );

    // The publisher lost the response, and by the time it retries its staged
    // archives are gone. The retry is the same request, so it gets the
    // recorded outcome without reading them.
    std::fs::remove_dir_all(&staging).unwrap();
    let retry = send_json(&app, "PUT", "/-/pnpr/v0/publish", &alice, &batch).await;
    assert_eq!(retry.status, StatusCode::CREATED, "{}", retry.text);
    assert_eq!(
        serde_json::from_str::<Value>(&retry.text).unwrap()["publication"],
        publication["id"],
    );
}

#[tokio::test]
async fn a_batch_referencing_an_archive_that_is_not_staged_publishes_nothing() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let alice = token_of(&app, "alice").await;
    let bob = token_of(&app, "bob").await;
    let core = npm_tgz("core", "2.0.0", b"core");
    let widgets = npm_tgz("widgets", "2.0.0", b"widgets");
    stage(&app, &core, &alice).await;
    // Staged, but by another account, which is the same as not staged.
    stage(&app, &widgets, &bob).await;
    let batch = json!({
        "packages": [staged_doc("core", "2.0.0", &core), staged_doc("widgets", "2.0.0", &widgets)],
    });

    let refused = send_json(&app, "PUT", "/-/pnpr/v0/publish", &alice, &batch).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert!(refused.text.contains("widgets-2.0.0.tgz"), "{}", refused.text);
    assert!(refused.text.contains("has not staged or which has expired"), "{}", refused.text);
    let (status, _) = get(&app, "/npm/core", &alice).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "nothing of the batch is published");
}

#[tokio::test]
async fn a_staged_archive_must_match_the_version_it_is_published_as() {
    let tmp = TempDir::new().unwrap();
    let app = router(config(tmp.path()));
    let alice = token_of(&app, "alice").await;
    let tgz = npm_tgz("mismatched", "1.0.0", b"payload");
    let other = npm_tgz("mismatched", "1.0.0", b"other payload");
    stage(&app, &other, &alice).await;
    let mut doc = staged_doc("mismatched", "1.0.0", &tgz);
    doc["_attachments"]["mismatched-1.0.0.tgz"]["integrity"] = json!(sri_sha512(&other));

    let refused = send_json(&app, "PUT", "/npm/mismatched", &alice, &doc).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert!(refused.text.contains("EINTEGRITY"), "{}", refused.text);
}

#[tokio::test]
async fn a_publish_held_for_approval_carries_its_archive() {
    let tmp = TempDir::new().unwrap();
    let app = router(config(tmp.path()));
    let alice = token_of(&app, "alice").await;
    let tgz = npm_tgz("held", "1.0.0", b"payload");
    stage(&app, &tgz, &alice).await;

    let refused = send_json(
        &app,
        "POST",
        "/npm/-/stage/package/held",
        &alice,
        &staged_doc("held", "1.0.0", &tgz),
    )
    .await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert!(refused.text.contains("inline"), "{}", refused.text);
}
