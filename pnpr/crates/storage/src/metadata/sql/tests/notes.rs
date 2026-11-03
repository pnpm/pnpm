//! Notes stored with a publication and changed after it.

use super::{Db, npm, packument, storage, write};
use crate::metadata::{
    BatchDocument, NoteChange, PublishNotes, ReleaseRecords, VersionNote, note_digest,
};
use pnpr_error::RegistryError;
use pnpr_package_name::Ecosystem;

fn change(text: Option<&str>) -> NoteChange {
    NoteChange { text: text.map(str::to_string), principal: "bob".to_string() }
}

async fn notes_published_with_a_release_keep_their_initial_digest(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let document = BatchDocument {
        ecosystem: Ecosystem::Npm,
        name: "core".to_string(),
        bytes: serde_json::to_vec(&packument("core", &["1.0.0"])).unwrap(),
        expected: None,
        principal: None,
        records: ReleaseRecords::new(),
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
    // A deleted note stays listed, for the digest it was published with.
    let listed = database
        .noted_versions(&npm("core"), "")
        .await
        .unwrap();
    assert_eq!(listed, ["1.0.0"]);
    let deleted = database
        .version_note(&npm("core"), "", "1.0.0")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(deleted.text, None);
    assert_eq!(deleted.initial_digest, Some(note_digest("first\n")));
}

async fn a_note_needs_its_subject_and_passes_the_content_rules(db: Db) {
    let storage = storage(&db);
    write(&storage, "pkg", &packument("pkg", &["1.0.0"])).await;
    let database = storage.sql_metadata();
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

async fn a_publish_time_note_needs_a_version_the_commit_introduces(db: Db) {
    let storage = storage(&db);
    write(&storage, "core", &packument("core", &["1.0.0"])).await;
    let database = storage.sql_metadata();
    // The commit only moves a tag; 1.0.0 was introduced before it.
    let mut retagged = packument("core", &["1.0.0"]);
    retagged["dist-tags"]["next"] = serde_json::json!("1.0.0");
    let current = storage
        .read_hosted_document_for_update(&npm("core"))
        .await
        .unwrap();
    let document = BatchDocument {
        ecosystem: Ecosystem::Npm,
        name: "core".to_string(),
        bytes: serde_json::to_vec(&retagged).unwrap(),
        expected: current.map(|document| document.version),
        principal: None,
        records: ReleaseRecords::new(),
    };
    let note = VersionNote {
        ecosystem: Ecosystem::Npm,
        name: "core".to_string(),
        version: "1.0.0".to_string(),
        text: "backdated\n".to_string(),
    };
    let notes = PublishNotes { publication: None, versions: vec![note] };
    let refused = database.commit_publication("", vec![document], Vec::new(), notes, None).await;
    assert!(matches!(refused, Err(RegistryError::AdminConflict { .. })), "{refused:?}");
    let stored = database
        .version_note(&npm("core"), "", "1.0.0")
        .await
        .unwrap();
    assert!(stored.is_none());
    let tags = read_tags(&storage).await;
    assert!(tags.get("next").is_none(), "the refused commit moved no tag");
}

async fn read_tags(storage: &crate::Storage) -> serde_json::Value {
    super::read(storage, "core").await["dist-tags"].clone()
}

conformance! {
    notes_published_with_a_release_keep_their_initial_digest,
    a_note_needs_its_subject_and_passes_the_content_rules,
    a_publish_time_note_needs_a_version_the_commit_introduces,
}
