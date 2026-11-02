use super::{NotesReader, render_upgrade_notes};
use node_semver::Version;
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use pretty_assertions::assert_eq;
use serde_json::json;

fn version(text: &str) -> Version {
    text.parse().expect("valid version")
}

fn publication(id: &str) -> serde_json::Value {
    json!({ "org": "example", "id": id, "name": format!("release-{id}") })
}

/// Reads `pkg`'s notes for an upgrade from 1.0.0 to `target`.
async fn upgrade_notes(server: &mockito::Server, target: &str) -> String {
    let config = Config::default();
    let client = ThrottledClient::default();
    let reader = NotesReader::new(&client, &config);
    let notes = reader
        .upgrade_notes(&format!("{}/", server.url()), "pkg", &version("1.0.0"), &version(target))
        .await
        .expect("notes read");
    render_upgrade_notes(&notes)
}

#[tokio::test]
async fn lists_the_range_newest_first_with_each_publication_once() {
    let mut server = mockito::Server::new_async().await;
    let list = json!({
        "notes": [
            { "version": "2.0.0-beta.1", "text": "beta" },
            { "version": "1.2.0", "text": "one two", "publication": publication("p2") },
            { "version": "1.1.0", "publication": publication("p1") },
            { "version": "1.0.1", "text": "one zero one", "publication": publication("p1") },
            { "version": "1.0.0", "text": "installed" },
        ],
    });
    let _list = server
        .mock("GET", "/pkg/-/notes")
        .with_body(list.to_string())
        .create_async()
        .await;
    let p1 = server
        .mock("GET", "/-/publications/example/p1/notes")
        .with_body("first release")
        .expect(1)
        .create_async()
        .await;
    let _p2 = server
        .mock("GET", "/-/publications/example/p2/notes")
        .with_status(404)
        .create_async()
        .await;

    let rendered = upgrade_notes(&server, "1.2.0").await;

    p1.assert_async().await;
    assert_eq!(
        rendered,
        "1.2.0:\n  one two\n\nRelease release-p1:\n  first release\n\n1.0.1:\n  one zero one",
    );
}

#[tokio::test]
async fn a_prerelease_target_includes_prereleases() {
    let mut server = mockito::Server::new_async().await;
    let list = json!({ "notes": [
        { "version": "2.0.0-beta.1", "text": "beta" },
        { "version": "1.2.0", "text": "stable" },
    ] });
    let _list = server
        .mock("GET", "/pkg/-/notes")
        .with_body(list.to_string())
        .create_async()
        .await;

    assert_eq!(
        upgrade_notes(&server, "2.0.0-beta.1").await,
        "2.0.0-beta.1:\n  beta\n\n1.2.0:\n  stable",
    );
}

#[tokio::test]
async fn reads_versions_past_a_truncated_list_one_at_a_time() {
    let mut server = mockito::Server::new_async().await;
    let list = json!({ "notes": [{ "version": "1.2.0", "text": "listed" }], "truncated": true });
    let _list = server
        .mock("GET", "/pkg/-/notes")
        .with_body(list.to_string())
        .create_async()
        .await;
    let packument = json!({
        "name": "pkg",
        "dist-tags": { "latest": "1.2.0" },
        "versions": {
            "1.0.0": { "name": "pkg", "version": "1.0.0", "dist": { "tarball": "https://example.com/a.tgz" } },
            "1.1.0": { "name": "pkg", "version": "1.1.0", "dist": { "tarball": "https://example.com/b.tgz" } },
            "1.2.0": { "name": "pkg", "version": "1.2.0", "dist": { "tarball": "https://example.com/c.tgz" } },
        },
    });
    let _packument = server
        .mock("GET", "/pkg")
        .with_body(packument.to_string())
        .create_async()
        .await;
    let older = server
        .mock("GET", "/pkg/1.1.0/notes")
        .with_header("link", r#"</-/publications/example/p1/notes>; rel="publication-notes""#)
        .with_body("older")
        .expect(1)
        .create_async()
        .await;
    let _p1 = server
        .mock("GET", "/-/publications/example/p1/notes")
        .with_body("p1")
        .create_async()
        .await;

    let rendered = upgrade_notes(&server, "1.2.0").await;

    older.assert_async().await;
    assert_eq!(
        rendered,
        "1.2.0:\n  listed\n\nRelease notes of the publication:\n  p1\n\n1.1.0:\n  older",
    );
}

#[tokio::test]
async fn a_registry_without_notes_has_none() {
    let mut server = mockito::Server::new_async().await;
    let _list = server
        .mock("GET", "/pkg/-/notes")
        .with_status(404)
        .create_async()
        .await;

    assert_eq!(upgrade_notes(&server, "1.2.0").await, "");
}

#[tokio::test]
async fn a_publication_link_to_another_origin_is_not_followed() {
    let mut server = mockito::Server::new_async().await;
    let _note = server
        .mock("GET", "/pkg/1.1.0/notes")
        .with_header("link", r#"<https://elsewhere.example/notes>; rel="publication-notes""#)
        .with_body("note")
        .create_async()
        .await;
    let config = Config::default();
    let client = ThrottledClient::default();
    let notes = NotesReader::new(&client, &config)
        .version_notes(&format!("{}/", server.url()), "pkg", "1.1.0")
        .await
        .expect("notes read");

    assert_eq!(render_upgrade_notes(&notes), "1.1.0:\n  note");
}
