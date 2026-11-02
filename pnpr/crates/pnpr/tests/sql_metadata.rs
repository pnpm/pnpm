//! The npm surface of a registry whose metadata lives in `SQLite`: publish,
//! read, retag, deprecate, and unpublish, with no package document written
//! to the hosted store.

use crate::npm::publish_doc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use pnpr::{Config, MaxUsers, router};
use pnpr_config::MetadataBackend;
use pnpr_fixtures::MockConfigPresets;
use pnpr_storage::layout::{local_blob_path, local_document_path};
use serde_json::{Value, json};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::Path,
};
use tempfile::TempDir;
use tower::ServiceExt;

fn sqlite_config(storage: &Path) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::static_serve(listen, storage.to_path_buf());
    config.http.public_url = "http://example.test".to_string();
    config.identity.auth.htpasswd.max_users = MaxUsers::Unlimited;
    config.storage.metadata = MetadataBackend::Sqlite(storage.join("metadata.sqlite"));
    config
}

fn request(method: &str, path: &str, token: &str, body: Option<&Value>) -> Request<Body> {
    let body = body.map_or_else(Body::empty, |body| Body::from(serde_json::to_vec(body).unwrap()));
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(body)
        .unwrap()
}

async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Option<String>, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(request)
        .await
        .unwrap();
    let generation = response
        .headers()
        .get("pnpr-generation")
        .map(|value| value.to_str().unwrap().to_string());
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap().to_vec();
    (status, generation, body)
}

async fn token(app: &Router) -> String {
    let body = json!({ "name": "alice", "password": "secret", "type": "user", "roles": [] });
    let (status, _, bytes) =
        send(app, request("PUT", "/-/user/org.couchdb.user:alice", "-", Some(&body))).await;
    assert_eq!(status, StatusCode::CREATED);
    serde_json::from_slice::<Value>(&bytes).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn packument(app: &Router, token: &str) -> (Option<String>, Value) {
    let (status, generation, bytes) = send(app, request("GET", "/npm/sql-pkg", token, None)).await;
    assert_eq!(status, StatusCode::OK);
    (generation, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn npm_metadata_round_trips_through_sqlite() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    for version in ["1.0.0", "1.1.0"] {
        let doc = publish_doc("sql-pkg", version, version.as_bytes());
        let (status, ..) = send(&app, request("PUT", "/npm/sql-pkg", &token, Some(&doc))).await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (generation, document) = packument(&app, &token).await;
    assert_eq!(generation.as_deref(), Some(":2"));
    assert_eq!(document["dist-tags"]["latest"], "1.1.0");
    assert!(!local_document_path(tmp.path(), "", pnpr_storage::Ecosystem::Npm, "sql-pkg").exists());
    let blob = local_blob_path(
        tmp.path(),
        "",
        pnpr_storage::Ecosystem::Npm,
        "sql-pkg",
        "sql-pkg-1.0.0.tgz",
    );
    assert_eq!(std::fs::read(blob).unwrap(), b"1.0.0");

    let tag = json!("1.0.0");
    let (status, committed, _) =
        send(&app, request("PUT", "/npm/-/package/sql-pkg/dist-tags/stable", &token, Some(&tag)))
            .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(committed.as_deref(), Some(":3"));
    let (generation, mut document) = packument(&app, &token).await;
    assert_eq!(generation, committed);
    assert_eq!(document["dist-tags"]["stable"], "1.0.0");

    document["versions"]["1.0.0"]["deprecated"] = json!("use 1.1.0");
    let (status, ..) =
        send(&app, request("PUT", "/npm/sql-pkg/-rev/1", &token, Some(&document))).await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, document) = packument(&app, &token).await;
    assert_eq!(document["versions"]["1.0.0"]["deprecated"], "use 1.1.0");
    assert!(document["versions"]["1.1.0"].get("deprecated").is_none());
}

#[tokio::test]
async fn a_restarted_registry_serves_and_unpublishes_from_sqlite() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    let doc = publish_doc("sql-pkg", "1.0.0", b"bytes");
    let (status, ..) = send(&app, request("PUT", "/npm/sql-pkg", &token, Some(&doc))).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, ..) = send(&app, request("PUT", "/npm/sql-pkg", &token, Some(&doc))).await;
    assert_eq!(status, StatusCode::CONFLICT, "a version is published once");

    // A static registry keeps its accounts in memory, so the restarted one
    // signs alice up again.
    let restarted = router(sqlite_config(tmp.path()));
    let token = self::token(&restarted).await;
    let (_, document) = packument(&restarted, &token).await;
    assert_eq!(document["versions"]["1.0.0"]["version"], "1.0.0");
    let (status, ..) =
        send(&restarted, request("DELETE", "/npm/sql-pkg/-rev/1", &token, None)).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, ..) = send(&restarted, request("GET", "/npm/sql-pkg", &token, None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
