//! Notes stored with a publication and changed after it.

use super::{npm, packument, storage, write};
use crate::metadata::{BatchDocument, NoteChange, PublishNotes, VersionNote, note_digest};
use pnpr_error::RegistryError;
use pnpr_package_name::Ecosystem;
use tempfile::TempDir;

fn change(text: Option<&str>) -> NoteChange {
    NoteChange { text: text.map(str::to_string), principal: "bob".to_string() }
}

#[tokio::test]
async fn notes_published_with_a_release_keep_their_initial_digest() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    let database = storage.sqlite_metadata().unwrap();
    let document = BatchDocument {
        ecosystem: Ecosystem::Npm,
        name: "core".to_string(),
        bytes: serde_json::to_vec(&packument("core", &["1.0.0"])).unwrap(),
        expected: None,
    };
    let version_note = VersionNote {
        ecosystem: Ecosystem::Npm,
        name: "core".to_string(),
        version: "1.0.0".to_string(),
        text: "first\n".to_string(),
    };
    let notes =
        PublishNotes { publication: Some("release\n".to_string()), versions: vec![version_note] };
    let committed = database
        .commit_publication("", vec![document], Vec::new(), notes, None)
        .await
        .unwrap();
    let id = committed.publication.unwrap();
    let release = database
        .publication_note("", id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(release.initial_digest, Some(note_digest("release\n")));

    assert!(
        database
            .set_version_note(&npm("core"), "", "1.0.0", change(Some("fixed\n")))
            .await
            .unwrap(),
    );
    let note = database
        .version_note(&npm("core"), "", "1.0.0")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(note.text.as_deref(), Some("fixed\n"));
    assert_eq!(note.principal.as_deref(), Some("bob"));
    assert_eq!(note.initial_digest, Some(note_digest("first\n")));

    assert!(
        database
            .set_version_note(&npm("core"), "", "1.0.0", change(None))
            .await
            .unwrap(),
    );
    let listed = database
        .package_notes(&npm("core"), "")
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].1.text, None);
    assert_eq!(listed[0].1.initial_digest, Some(note_digest("first\n")));
}

#[tokio::test]
async fn a_note_needs_its_subject_and_passes_the_content_rules() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    write(&storage, "pkg", &packument("pkg", &["1.0.0"])).await;
    let database = storage.sqlite_metadata().unwrap();
    assert!(
        !database
            .set_version_note(&npm("pkg"), "", "2.0.0", change(Some("x")))
            .await
            .unwrap(),
    );
    assert!(
        !database
            .set_publication_note("", 42, change(Some("x")))
            .await
            .unwrap(),
    );
    let refused = database.set_version_note(&npm("pkg"), "", "1.0.0", change(Some("a\rb"))).await;
    assert!(matches!(refused, Err(RegistryError::BadRequest { .. })), "{refused:?}");
    assert!(
        database
            .version_note(&npm("pkg"), "", "1.0.0")
            .await
            .unwrap()
            .is_none(),
    );
}
