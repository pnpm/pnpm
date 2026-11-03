//! Staging release archives by integrity ahead of the publish commit that
//! references them: `PUT` and `HEAD /-/pnpr/v0/artifacts/blob?integrity=...`
//! with no owner.

use crate::npm::{npm_tgz, sri_sha512};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use pnpr::{Config, MaxUsers, router};
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
