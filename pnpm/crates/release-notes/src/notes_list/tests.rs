use super::{NotesList, PublicationRef, is_in_upgrade_range};
use node_semver::Version;
use pretty_assertions::assert_eq;

fn version(text: &str) -> Version {
    text.parse().expect("valid version")
}

fn in_range(candidate: &str, installed: &str, target: &str) -> bool {
    is_in_upgrade_range(&version(candidate), &version(installed), &version(target))
}

#[test]
fn a_stable_target_skips_prereleases_in_between() {
    assert!(in_range("1.3.0", "1.2.0", "1.4.0"));
    assert!(in_range("1.4.0", "1.2.0", "1.4.0"));
    assert!(!in_range("1.2.0", "1.2.0", "1.4.0"));
    assert!(!in_range("1.5.0", "1.2.0", "1.4.0"));
    assert!(!in_range("1.4.0-beta.1", "1.2.0", "1.4.0"));
}

#[test]
fn a_prerelease_target_includes_prereleases() {
    assert!(in_range("2.0.0-alpha.1", "1.2.0", "2.0.0-alpha.2"));
    assert!(in_range("2.0.0-alpha.2", "1.2.0", "2.0.0-alpha.2"));
    assert!(in_range("1.3.0", "1.2.0", "2.0.0-alpha.2"));
    assert!(!in_range("2.0.0", "1.2.0", "2.0.0-alpha.2"));
}

#[test]
fn parses_the_rfc_example_document() {
    let list: NotesList = serde_json::from_str(
        r#"{
          "notes": [
            {
              "version": "12.0.0",
              "modified": "2026-10-28T09:12:44Z",
              "text": "- `install` reads lockfiles from 11.x.\n",
              "initialDigest": "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=",
              "publication": { "org": "example", "id": "9f2c41", "name": "v12.0.0" }
            },
            { "version": "11.4.0", "modified": "2026-09-30T17:01:02Z" }
          ],
          "truncated": true
        }"#,
    )
    .expect("parse notes list");
    assert!(list.truncated);
    assert_eq!(
        list.notes[0].publication
            .as_ref()
            .and_then(PublicationRef::notes_path)
            .as_deref(),
        Some("-/publications/example/9f2c41/notes"),
    );
    assert_eq!(list.notes[1].text, None);
}

#[test]
fn refuses_a_publication_path_that_is_not_one_segment() {
    for (org, id) in [("example", ".."), ("ex/ample", "1"), ("example", "a%2fb"), ("", "1")] {
        let publication =
            PublicationRef { org: org.into(), id: id.into(), name: None, initial_digest: None };
        assert_eq!(publication.notes_path(), None, "{org}/{id}");
    }
}
