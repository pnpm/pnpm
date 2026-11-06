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
    app_at(tmp, "http://example.test")
}

/// A registry clients reach at `public_url`.
fn app_at(tmp: &TempDir, public_url: &str) -> Router {
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::static_serve(listen, tmp.path().to_path_buf());
    config.http.public_url = public_url.to_string();
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
    assert_eq!(listing["notes"][0]["publication"]["url"], "/-/publications/-/1/notes");
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

#[tokio::test]
async fn a_publication_note_url_keeps_the_public_url_path_prefix() {
    let tmp = TempDir::new().unwrap();
    // A reverse proxy serves pnpr under `/registry/` and strips the prefix.
    let app = app_at(&tmp, "https://example.test/registry/");
    let token = token(&app).await;
    let batch = json!({
        "name": "v1.0.0",
        "notes": "## The release\n",
        "packages": [publish_doc("widgets", "1.0.0", b"widgets")],
    });
    let published = send(&app, "PUT", "/-/pnpr/v0/publish", Some(&token), json_body(&batch)).await;
    assert_eq!(published.status, StatusCode::CREATED);

    let widgets = send(&app, "GET", "/npm/widgets/1.0.0/notes", None, Body::empty()).await;
    assert_eq!(
        widgets.header("link"),
        Some(r#"</registry/-/publications/-/1/notes>; rel="publication-notes""#),
    );
    let listing = send(&app, "GET", "/npm/widgets/-/notes", None, Body::empty())
        .await
        .json();
    assert_eq!(listing["notes"][0]["publication"]["url"], "/registry/-/publications/-/1/notes");
}

#[tokio::test]
async fn an_unpublished_version_serves_no_note() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let token = token(&app).await;
    for version in ["1.0.0", "1.1.0"] {
        let doc = with_notes(publish_doc("pruned", version, version.as_bytes()), "- note\n");
        let published = send(&app, "PUT", "/npm/pruned", Some(&token), json_body(&doc)).await;
        assert_eq!(published.status, StatusCode::CREATED);
    }
    let mut packument = send(&app, "GET", "/npm/pruned", Some(&token), Body::empty())
        .await
        .json();
    packument["versions"]
        .as_object_mut()
        .unwrap()
        .remove("1.0.0");
    let rewritten = send(
        &app,
        "PUT",
        &format!("/npm/pruned/-rev/{}", packument["_rev"].as_str().unwrap()),
        Some(&token),
        json_body(&packument),
    )
    .await;
    assert_eq!(rewritten.status, StatusCode::CREATED, "{}", rewritten.text());

    let removed = send(&app, "GET", "/npm/pruned/1.0.0/notes", None, Body::empty()).await;
    assert_eq!(removed.status, StatusCode::NOT_FOUND);
    let kept = send(&app, "GET", "/npm/pruned/1.1.0/notes", None, Body::empty()).await;
    assert_eq!(kept.status, StatusCode::OK);
    let listing = send(&app, "GET", "/npm/pruned/-/notes", None, Body::empty())
        .await
        .json();
    let versions: Vec<&str> = listing["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["version"].as_str().unwrap())
        .collect();
    assert_eq!(versions, ["1.1.0"]);
}

#[tokio::test]
async fn a_version_spelled_dist_tag_names_the_version_it_points_at() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let token = token(&app).await;
    let doc = with_notes(publish_doc("tagged", "1.0.0", b"bytes"), "- one\n");
    let published = send(&app, "PUT", "/npm/tagged", Some(&token), json_body(&doc)).await;
    assert_eq!(published.status, StatusCode::CREATED);
    let tag = send(
        &app,
        "PUT",
        "/npm/-/package/tagged/dist-tags/v9.9.9",
        Some(&token),
        Body::from(r#""1.0.0""#),
    )
    .await;
    assert_eq!(tag.status, StatusCode::CREATED, "{}", tag.text());
    let note = send(&app, "GET", "/npm/tagged/v9.9.9/notes", None, Body::empty()).await;
    assert_eq!((note.status, note.text()), (StatusCode::OK, "- one\n"));
}

#[tokio::test]
async fn a_publish_that_only_brings_notes_is_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let token = token(&app).await;
    let doc = publish_doc("quiet", "1.0.0", b"bytes");
    let published = send(&app, "PUT", "/npm/quiet", Some(&token), json_body(&doc)).await;
    assert_eq!(published.status, StatusCode::CREATED);
    let noted = with_notes(doc, "- late\n");
    let repeated = send(&app, "PUT", "/npm/quiet", Some(&token), json_body(&noted)).await;
    assert_eq!(repeated.status, StatusCode::CONFLICT, "{}", repeated.text());
    let note = send(&app, "GET", "/npm/quiet/1.0.0/notes", None, Body::empty()).await;
    assert_eq!(note.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_note_body_is_bounded_by_the_note_size() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let token = token(&app).await;
    let doc = publish_doc("bounded", "1.0.0", b"bytes");
    let published = send(&app, "PUT", "/npm/bounded", Some(&token), json_body(&doc)).await;
    assert_eq!(published.status, StatusCode::CREATED);
    let oversized = Body::from("x".repeat(1024 * 1024));
    let refused = send(&app, "PUT", "/npm/bounded/1.0.0/notes", None, oversized).await;
    assert_eq!(refused.status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn a_scoped_version_manifest_answers_head() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let token = token(&app).await;
    let doc = publish_doc("@scope/headed", "1.0.0", b"bytes");
    let published = send(&app, "PUT", "/npm/@scope%2fheaded", Some(&token), json_body(&doc)).await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.text());
    let manifest = send(&app, "HEAD", "/npm/@scope/headed/1.0.0", None, Body::empty()).await;
    assert_eq!(manifest.status, StatusCode::OK);
}

#[tokio::test]
async fn a_package_of_an_upstream_lists_no_notes() {
    let tmp = TempDir::new().unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::proxy(listen, tmp.path().to_path_buf());
    config.storage.metadata = MetadataBackend::Sqlite(tmp.path().join("metadata.sqlite"));
    let app = router(config);
    let listing = send(&app, "GET", "/npm/lodash/-/notes", None, Body::empty()).await;
    assert_eq!(listing.status, StatusCode::OK, "{}", listing.text());
    assert_eq!(listing.json(), json!({ "notes": [] }));
    let note = send(&app, "GET", "/npm/lodash/4.17.21/notes", None, Body::empty()).await;
    assert_eq!(note.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_note_added_and_deleted_after_publishing_is_not_listed() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let token = token(&app).await;
    let doc = publish_doc("erased", "1.0.0", b"bytes");
    let published = send(&app, "PUT", "/npm/erased", Some(&token), json_body(&doc)).await;
    assert_eq!(published.status, StatusCode::CREATED);
    let added = send(&app, "PUT", "/npm/erased/1.0.0/notes", Some(&token), Body::from("x")).await;
    assert_eq!(added.status, StatusCode::NO_CONTENT);
    let deleted =
        send(&app, "DELETE", "/npm/erased/1.0.0/notes", Some(&token), Body::empty()).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    let listing = send(&app, "GET", "/npm/erased/-/notes", None, Body::empty())
        .await
        .json();
    assert_eq!(listing, json!({ "notes": [] }));
}

#[tokio::test]
async fn a_response_that_withholds_a_publication_note_is_not_shared() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let token = token(&app).await;
    // Only signed-in callers may read `@private/*`, so an anonymous caller
    // may not read every member of the publication.
    let batch = json!({
        "notes": "## The release\n",
        "packages": [
            publish_doc("@private/inner", "1.0.0", b"inner"),
            with_notes(publish_doc("@foo/outer", "1.0.0", b"outer"), "outer changes\n"),
        ],
    });
    let published = send(&app, "PUT", "/-/pnpr/v0/publish", Some(&token), json_body(&batch)).await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.text());

    for path in ["/npm/@foo/outer/1.0.0/notes", "/npm/@foo/outer/-/notes"] {
        let anonymous = send(&app, "GET", path, None, Body::empty()).await;
        assert_eq!(anonymous.status, StatusCode::OK, "{path}");
        assert_eq!(anonymous.header("cache-control"), Some("private, no-store"), "{path}");
        let signed_in = send(&app, "GET", path, Some(&token), Body::empty()).await;
        assert_eq!(signed_in.header("cache-control"), Some("private, no-store"), "{path}");
    }
    let anonymous = send(&app, "GET", "/npm/@foo/outer/1.0.0/notes", None, Body::empty()).await;
    assert_eq!(anonymous.header("link"), None);
}

#[tokio::test]
async fn a_publication_note_added_and_deleted_after_publishing_is_not_linked() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let token = token(&app).await;
    let batch = json!({ "name": "v1", "packages": [publish_doc("unlinked", "1.0.0", b"bytes")] });
    let published = send(&app, "PUT", "/-/pnpr/v0/publish", Some(&token), json_body(&batch)).await;
    assert_eq!(published.status, StatusCode::CREATED);
    let path = "/-/publications/-/v1/notes";
    let added = send(&app, "PUT", path, Some(&token), Body::from("late\n")).await;
    assert_eq!(added.status, StatusCode::NO_CONTENT);
    let deleted = send(&app, "DELETE", path, Some(&token), Body::empty()).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);

    let note = send(&app, "GET", "/npm/unlinked/1.0.0/notes", None, Body::empty()).await;
    assert_eq!(note.status, StatusCode::NOT_FOUND);
    assert_eq!(note.header("link"), None);
}
