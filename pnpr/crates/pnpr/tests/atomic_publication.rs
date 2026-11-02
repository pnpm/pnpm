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
use pnpr_policy::{AccessList, PackageRules, RuleOverride};
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
    text: String,
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
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Answer { status, generation, body, text }
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

    // The whole request is the key: the same archive under another tag is a
    // new request, not a replay of the committed one.
    let mut other_tag = doc.clone();
    other_tag["dist-tags"] = json!({ "beta": "1.0.0" });
    let retagged = send(&app, "PUT", "/npm/retried", &token, Some(&other_tag)).await;
    assert_eq!(retagged.status, StatusCode::CONFLICT);
}

/// Leave `bob` alone allowed to publish, or with `revoke_read`, to read.
fn only_bob(rules: &PackageRules, revoke_read: bool) {
    let bob = AccessList::from_tokens(["bob"]);
    let lists = if revoke_read {
        RuleOverride { access: Some(bob.clone()), ..RuleOverride::default() }
    } else {
        RuleOverride { publish: Some(bob.clone()), ..RuleOverride::default() }
    };
    let table = rules.snapshot();
    let overrides = table
        .rules()
        .iter()
        .map(|rule| (rule.pattern.clone(), lists.clone()))
        .collect();
    let changed = table
        .with_overrides(revoke_read.then(|| bob.clone()), overrides)
        .unwrap();
    rules.replace(if revoke_read { changed } else { changed.with_default_publish(bob) });
}

#[tokio::test]
async fn a_recorded_outcome_needs_read_access_but_not_publish_permission() {
    let tmp = TempDir::new().unwrap();
    let config = sqlite_config(tmp.path());
    let rules = config.routing.hosted["local"].rules.clone();
    let app = router(config);
    let token = token(&app).await;
    let doc = publish_doc("revoked", "1.0.0", b"bytes");
    let first = send(&app, "PUT", "/npm/revoked", &token, Some(&doc)).await;
    assert_eq!(first.status, StatusCode::CREATED);

    only_bob(&rules, false);
    let retry = send(&app, "PUT", "/npm/revoked", &token, Some(&doc)).await;
    assert_eq!((retry.status, retry.generation), (StatusCode::CREATED, first.generation));

    only_bob(&rules, true);
    let unreadable = send(&app, "PUT", "/npm/revoked", &token, Some(&doc)).await;
    assert_ne!(unreadable.status, StatusCode::CREATED);
    assert_eq!(unreadable.generation, None);
    assert_eq!(unreadable.body.get("publication"), None);
}

#[tokio::test]
async fn a_publication_name_is_never_a_decimal_number() {
    let tmp = TempDir::new().unwrap();
    let app = router(sqlite_config(tmp.path()));
    let token = token(&app).await;
    let batch = json!({ "name": "12", "packages": [publish_doc("numbered", "1.0.0", b"bytes")] });
    let refused = send(&app, "PUT", "/-/pnpr/v0/publish", &token, Some(&batch)).await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
    assert!(refused.text.contains("decimal number"), "{}", refused.text);
    let numbered = send(&app, "GET", "/npm/numbered", &token, None).await;
    assert_eq!(numbered.status, StatusCode::NOT_FOUND);
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
