//! The npm surface of a registry whose metadata lives in `SQLite`: publish,
//! read, retag, deprecate, and unpublish, with no package document written
//! to the hosted store.

use crate::npm::{npm_tgz, publish_doc, sri_sha512};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use pnpr::{Config, MaxUsers, router};
use pnpr_config::MetadataBackend;
use pnpr_fixtures::MockConfigPresets;
use pnpr_storage::{
    TOMBSTONES_NAMESPACE,
    layout::{local_blob_path, local_document_path, namespace_segments, package_segments},
};
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
    token_of(app, "alice").await
}

async fn token_of(app: &Router, user: &str) -> String {
    let body = json!({ "name": user, "password": "secret", "type": "user", "roles": [] });
    let path = format!("/-/user/org.couchdb.user:{user}");
    let (status, _, bytes) = send(app, request("PUT", &path, "-", Some(&body))).await;
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
    let archive = npm_tgz("sql-pkg", "1.0.0", b"1.0.0");
    assert_eq!(std::fs::read(blob).unwrap(), archive);
    // The version is served as its archive was accepted, under the profile
    // the accepted manifest records.
    let accepted = &document["versions"]["1.0.0"];
    assert_eq!(accepted["_archiveManifest"]["profile"], "npm-install-manifest/1");
    let manifest = json!({ "name": "sql-pkg", "version": "1.0.0" }).to_string();
    assert_eq!(accepted["_archiveManifest"]["packageJson"], sri_sha512(manifest.as_bytes()));
    assert_eq!(accepted["hasInstallScript"], false);
    assert_eq!(accepted["_hasShrinkwrap"], false);

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
    let other = publish_doc("sql-pkg", "1.0.0", b"other bytes");
    let (status, ..) = send(&app, request("PUT", "/npm/sql-pkg", &token, Some(&other))).await;
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

async fn publish(app: &Router, token: &str, version: &str) -> StatusCode {
    let doc = publish_doc("sql-pkg", version, version.as_bytes());
    send(app, request("PUT", "/npm/sql-pkg", token, Some(&doc))).await.0
}

fn tombstone_path(storage: &Path) -> std::path::PathBuf {
    let mut path = storage.to_path_buf();
    path.extend(namespace_segments("", pnpr_storage::Ecosystem::Npm));
    path.push(TOMBSTONES_NAMESPACE);
    path.push(format!(
        "{}.json",
        package_segments(pnpr_storage::Ecosystem::Npm, "sql-pkg").join("/"),
    ));
    path
}

#[tokio::test]
async fn an_unpublished_version_is_refused_even_without_its_tombstone() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    for version in ["1.0.0", "1.1.0"] {
        assert_eq!(publish(&app, &token, version).await, StatusCode::CREATED);
    }
    let (_, mut document) = packument(&app, &token).await;
    document["versions"]
        .as_object_mut()
        .unwrap()
        .remove("1.0.0");
    let (status, ..) =
        send(&app, request("PUT", "/npm/sql-pkg/-rev/1", &token, Some(&document))).await;
    assert_eq!(status, StatusCode::CREATED);

    // The metadata store keeps the removed version itself, so losing the
    // server's tombstone does not let it back in.
    std::fs::remove_file(tombstone_path(tmp.path())).unwrap();
    let (status, _, body) = send(
        &app,
        request("PUT", "/npm/sql-pkg", &token, Some(&publish_doc("sql-pkg", "1.0.0", b"other"))),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{}", String::from_utf8_lossy(&body));
    let (_, document) = packument(&app, &token).await;
    assert!(document["versions"].get("1.0.0").is_none());

    // Unpublishing the whole package keeps every version it had.
    let (status, ..) = send(&app, request("DELETE", "/npm/sql-pkg/-rev/1", &token, None)).await;
    assert_eq!(status, StatusCode::CREATED);
    std::fs::remove_file(tombstone_path(tmp.path())).unwrap();
    // Other bytes than the first publish of 1.1.0, so the request is not a
    // replay of that one.
    let republished = publish_doc("sql-pkg", "1.1.0", b"other");
    let (status, ..) = send(&app, request("PUT", "/npm/sql-pkg", &token, Some(&republished))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(publish(&app, &token, "2.0.0").await, StatusCode::CREATED);
}

#[cfg(unix)]
#[tokio::test]
async fn an_unpublish_whose_archive_removal_fails_leaves_the_name_tombstoned() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let alice = token(&app).await;
    assert_eq!(publish(&app, &alice, "1.0.0").await, StatusCode::CREATED);

    // A read-only archive directory, so removing its archives fails after the
    // package's metadata is gone.
    let archive = local_blob_path(
        tmp.path(),
        "",
        pnpr_storage::Ecosystem::Npm,
        "sql-pkg",
        "sql-pkg-1.0.0.tgz",
    );
    let archives = archive.parent().unwrap();
    std::fs::set_permissions(archives, std::fs::Permissions::from_mode(0o555)).unwrap();
    let (status, ..) = send(&app, request("DELETE", "/npm/sql-pkg/-rev/1", &alice, None)).await;
    std::fs::set_permissions(archives, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(status.is_server_error(), "the removal fails: {status}");
    assert!(archive.is_file());

    // The tombstone was written first, so the name is not free for anyone
    // but its former maintainer.
    assert!(tombstone_path(tmp.path()).is_file());
    let bob = token_of(&app, "bob").await;
    assert_eq!(publish(&app, &bob, "2.0.0").await, StatusCode::FORBIDDEN);
}

/// GET `path` with `accept`, as `token`: the status, the `Cache-Control`
/// header, and the body as JSON.
async fn get_json(
    app: &Router,
    path: &str,
    accept: &str,
    token: &str,
) -> (StatusCode, Option<String>, Value) {
    let mut request = request("GET", path, token, None);
    request
        .headers_mut()
        .insert(header::ACCEPT, accept.parse().unwrap());
    let response = app
        .clone()
        .oneshot(request)
        .await
        .unwrap();
    let cache_control = response
        .headers()
        .get(header::CACHE_CONTROL)
        .map(|value| value.to_str().unwrap().to_string());
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, cache_control, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn pnprs_package_document_and_release_records_render_from_sqlite() {
    const INSTALL_V2: &str = "application/vnd.pnpr.install-v2+json";
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    let (_, tgz) = crate::server::package_tar_and_tgz(r#"{"name":"sql-pkg","version":"1.0.0"}"#);
    let doc = publish_doc("sql-pkg", "1.0.0", &tgz);
    let (status, ..) = send(&app, request("PUT", "/npm/sql-pkg", &token, Some(&doc))).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _, document) = get_json(&app, "/npm/sql-pkg", INSTALL_V2, &token).await;
    assert_eq!(status, StatusCode::OK);
    let entry = &document["versions"]["1.0.0"];
    assert_eq!(entry["requiresBuild"], json!(false));
    assert!(entry["manifest"].is_object(), "{entry}");

    // pnpr derived every fact about the archive at publish, so the record is
    // cached forever.
    let record_path = "/npm/sql-pkg/1.0.0+r0";
    let (status, cache_control, record) =
        get_json(&app, record_path, "application/json", &token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control.as_deref(), Some("public, max-age=31536000, immutable"));
    assert_eq!(record["name"], "sql-pkg");
    assert_eq!(record["version"], "1.0.0");
    assert_eq!(record["integrity"], entry["integrity"]);
    assert_eq!(record["requiresBuild"], json!(false));

    // A deprecation changes the next document and leaves the record alone.
    let (_, mut packument) = packument(&app, &token).await;
    packument["versions"]["1.0.0"]["deprecated"] = json!("use 2.0.0");
    let (status, ..) =
        send(&app, request("PUT", "/npm/sql-pkg/-rev/1", &token, Some(&packument))).await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, _, document) = get_json(&app, "/npm/sql-pkg", INSTALL_V2, &token).await;
    assert_eq!(document["versions"]["1.0.0"]["deprecated"], "use 2.0.0");
    let (_, _, unchanged) = get_json(&app, record_path, "application/json", &token).await;
    assert_eq!(unchanged, record);
}
