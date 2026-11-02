//! Publishing to a registry whose metadata lives in `SQLite`: a retried
//! publish gets its recorded outcome, a batch commits as one publication, and
//! publications are served under `/-/publications/`.

use crate::npm::publish_doc;

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

fn sqlite_config(storage: &Path) -> Config {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::static_serve(listen, storage.to_path_buf());
    config.http.public_url = "http://example.test".to_string();
    config.identity.auth.htpasswd.max_users = MaxUsers::Unlimited;
    config.storage.metadata = MetadataBackend::Sqlite(storage.join("metadata.sqlite"));
    config
}

struct Answer {
    status: StatusCode,
    generation: Option<String>,
    body: Value,
}

async fn send(app: &Router, method: &str, path: &str, token: &str, body: Option<&Value>) -> Answer {
    send_with_key(app, method, path, token, body, None).await
}

async fn send_with_key(
    app: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&Value>,
    key: Option<&str>,
) -> Answer {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    if let Some(key) = key {
        request = request.header("idempotency-key", key);
    }
    let body = body.map_or_else(Body::empty, |body| Body::from(serde_json::to_vec(body).unwrap()));
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let generation = response
        .headers()
        .get("pnpr-generation")
        .map(|value| value.to_str().unwrap().to_string());
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Answer { status, generation, body }
}

async fn token(app: &Router) -> String {
    let body = json!({ "name": "alice", "password": "secret", "type": "user", "roles": [] });
    let answer = send(app, "PUT", "/-/user/org.couchdb.user:alice", "-", Some(&body)).await;
    assert_eq!(answer.status, StatusCode::CREATED);
    answer.body["token"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn a_retried_publish_gets_its_recorded_outcome() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    let doc = publish_doc("retried", "1.0.0", b"bytes");
    let first = send(&app, "PUT", "/npm/retried", &token, Some(&doc)).await;
    assert_eq!(first.status, StatusCode::CREATED);
    assert_eq!(first.generation.as_deref(), Some(":1"));
    assert_eq!(first.body["publication"], 1);

    // The publisher never saw that response and sends the same request again.
    let retry = send(&app, "PUT", "/npm/retried", &token, Some(&doc)).await;
    assert_eq!(retry.status, StatusCode::CREATED);
    assert_eq!(retry.generation, first.generation);

    let other_archive = publish_doc("retried", "1.0.0", b"other bytes");
    let conflict = send(&app, "PUT", "/npm/retried", &token, Some(&other_archive)).await;
    assert_eq!(conflict.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_batch_is_one_publication() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    let batch = json!({
        "name": "v2.0.0",
        "packages": [publish_doc("core", "2.0.0", b"core"), publish_doc("widgets", "2.0.0", b"widgets")],
    });
    let committed =
        send_with_key(&app, "PUT", "/-/pnpr/v0/publish", &token, Some(&batch), Some("k1")).await;
    assert_eq!(committed.status, StatusCode::CREATED);
    assert_eq!(committed.generation.as_deref(), Some(":1"));

    let publication = send(&app, "GET", "/-/publications/-/v2.0.0", &token, None).await;
    assert_eq!(publication.status, StatusCode::OK);
    assert_eq!(publication.body["id"], 1);
    assert_eq!(publication.body["origin"], "published");
    assert_eq!(publication.body["publisher"], "alice");
    let members: Vec<&str> = publication.body["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| member["name"].as_str().unwrap())
        .collect();
    assert_eq!(members, ["core", "widgets"]);
    let by_member =
        send(&app, "GET", "/-/publications/-?member=npm:widgets@2.0.0", &token, None).await;
    assert_eq!(by_member.body["name"], "v2.0.0");
    let missing = send(&app, "GET", "/-/publications/-/v9", &token, None).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);

    let replay =
        send_with_key(&app, "PUT", "/-/pnpr/v0/publish", &token, Some(&batch), Some("k1")).await;
    assert_eq!((replay.status, replay.generation), (StatusCode::CREATED, committed.generation));
    let other = json!({ "packages": [publish_doc("gadgets", "1.0.0", b"gadgets")] });
    let reused =
        send_with_key(&app, "PUT", "/-/pnpr/v0/publish", &token, Some(&other), Some("k1")).await;
    assert_eq!(reused.status, StatusCode::CONFLICT);
    let gadgets = send(&app, "GET", "/npm/gadgets", &token, None).await;
    assert_eq!(gadgets.status, StatusCode::NOT_FOUND);
}
