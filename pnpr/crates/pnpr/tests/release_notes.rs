//! Release notes on a registry whose metadata lives in `SQLite`: notes sent
//! with a publish, edited later, and read per version, per package, and per
//! publication.

use crate::npm::publish_doc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode, header},
};
use pnpr::{Config, MaxUsers, router};
use pnpr_config::MetadataBackend;
use pnpr_fixtures::MockConfigPresets;
use serde_json::{Value, json};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use tempfile::TempDir;
use tower::ServiceExt;

fn app(tmp: &TempDir) -> Router {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::static_serve(listen, tmp.path().to_path_buf());
    config.http.public_url = "http://example.test".to_string();
    config.identity.auth.htpasswd.max_users = MaxUsers::Unlimited;
    config.storage.metadata = MetadataBackend::Sqlite(tmp.path().join("metadata.sqlite"));
    router(config)
}

struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Answer {
    fn text(&self) -> &str {
        std::str::from_utf8(&self.body).unwrap()
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(name)
            .map(|value| value.to_str().unwrap())
    }
}

async fn send(app: &Router, method: &str, path: &str, token: Option<&str>, body: Body) -> Answer {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    let body = to_bytes(body, usize::MAX).await.unwrap().to_vec();
    Answer { status: parts.status, headers: parts.headers, body }
}

fn json_body(value: &Value) -> Body {
    Body::from(serde_json::to_vec(value).unwrap())
}

async fn token(app: &Router) -> String {
    let body = json!({ "name": "alice", "password": "secret", "type": "user", "roles": [] });
    let answer = send(app, "PUT", "/-/user/org.couchdb.user:alice", None, json_body(&body)).await;
    assert_eq!(answer.status, StatusCode::CREATED);
    answer.json()["token"]
        .as_str()
        .unwrap()
        .to_string()
}

fn with_notes(mut doc: Value, notes: &str) -> Value {
    doc["notes"] = json!(notes);
    doc
}

#[tokio::test]
async fn a_version_note_is_published_edited_and_deleted() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let token = token(&app).await;
    let doc = with_notes(publish_doc("noted", "1.0.0", b"bytes"), "- first\n");
    let published = send(&app, "PUT", "/npm/noted", Some(&token), json_body(&doc)).await;
    assert_eq!(published.status, StatusCode::CREATED);
    let packument = send(&app, "GET", "/npm/noted", None, Body::empty()).await;
    assert!(packument.json().get("notes").is_none(), "a note is not part of the packument");

    let note = send(&app, "GET", "/npm/noted/latest/notes", None, Body::empty()).await;
    assert_eq!((note.status, note.text()), (StatusCode::OK, "- first\n"));
    assert_eq!(note.header("content-type"), Some("text/markdown; charset=utf-8"));
    assert_eq!(note.header("cache-control"), Some("public, max-age=300"));
    let initial = note
        .header("pnpr-note-initial-digest")
        .unwrap()
        .to_string();

    let edit =
        send(&app, "PUT", "/npm/noted/1.0.0/notes", Some(&token), Body::from("- fixed\n")).await;
    assert_eq!(edit.status, StatusCode::NO_CONTENT);
    let anonymous = send(&app, "PUT", "/npm/noted/1.0.0/notes", None, Body::from("x")).await;
    assert!(anonymous.status.is_client_error());
    let note = send(&app, "GET", "/npm/noted/1.0.0/notes", None, Body::empty()).await;
    assert_eq!(note.text(), "- fixed\n");
    assert_eq!(note.header("pnpr-note-initial-digest"), Some(initial.as_str()));

    let deleted = send(&app, "DELETE", "/npm/noted/1.0.0/notes", Some(&token), Body::empty()).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    let gone = send(&app, "GET", "/npm/noted/1.0.0/notes", None, Body::empty()).await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
    assert_eq!(gone.header("pnpr-note-initial-digest"), Some(initial.as_str()));
    let ahead = send(&app, "PUT", "/npm/noted/9.0.0/notes", Some(&token), Body::from("x")).await;
    assert_eq!(ahead.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_rejected_note_rejects_the_publish() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let token = token(&app).await;
    let doc = with_notes(publish_doc("tricky", "1.0.0", b"bytes"), "\u{1b}[2Jcleared");
    let published = send(&app, "PUT", "/npm/tricky", Some(&token), json_body(&doc)).await;
    assert_eq!(published.status, StatusCode::BAD_REQUEST);
    let packument = send(&app, "GET", "/npm/tricky", None, Body::empty()).await;
    assert_eq!(packument.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_publication_note_is_reached_from_each_member() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let token = token(&app).await;
    let batch = json!({
        "name": "v1.0.0",
        "notes": "## The release\n",
        "packages": [
            with_notes(publish_doc("@acme/core", "1.0.0", b"core"), "core changes\n"),
            publish_doc("widgets", "1.0.0", b"widgets"),
        ],
    });
    let published = send(&app, "PUT", "/-/pnpr/v0/publish", Some(&token), json_body(&batch)).await;
    assert_eq!(published.status, StatusCode::CREATED);

    let release = send(&app, "GET", "/-/publications/-/v1.0.0/notes", None, Body::empty()).await;
    assert_eq!((release.status, release.text()), (StatusCode::OK, "## The release\n"));
    let core = send(&app, "GET", "/npm/@acme/core/1.0.0/notes", None, Body::empty()).await;
    assert_eq!(core.text(), "core changes\n");
    let widgets = send(&app, "GET", "/npm/widgets/1.0.0/notes", None, Body::empty()).await;
    assert_eq!(widgets.status, StatusCode::NO_CONTENT);
    assert_eq!(
        widgets.header("link"),
        Some(r#"</-/publications/-/1/notes>; rel="publication-notes""#),
    );
    assert_eq!(widgets.header("pnpr-publication-name"), Some("v1.0.0"));

    let listing = send(&app, "GET", "/npm/widgets/-/notes", None, Body::empty())
        .await
        .json();
    assert_eq!(listing["notes"][0]["version"], "1.0.0");
    assert_eq!(listing["notes"][0]["publication"]["name"], "v1.0.0");
    assert!(listing["notes"][0].get("text").is_none());
    let scoped = send(&app, "GET", "/npm/@acme/core/-/notes", None, Body::empty())
        .await
        .json();
    assert_eq!(scoped["notes"][0]["text"], "core changes\n");

    let edit =
        send(&app, "PUT", "/-/publications/-/1/notes", Some(&token), Body::from("edited\n")).await;
    assert_eq!(edit.status, StatusCode::NO_CONTENT);
    let release = send(&app, "GET", "/-/publications/-/v1.0.0/notes", None, Body::empty()).await;
    assert_eq!(release.text(), "edited\n");
}
