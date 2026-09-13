use std::path::Path;

use indexmap::IndexMap;
use pretty_assertions::assert_eq;

use super::{IntentBumpType, parse_change_intent, read_change_intents, write_change_intent};

#[test]
fn parses_the_changesets_file_format() {
    let content = "---\n\"@example/ui\": minor\n\"@example/core\": patch\n---\n\nAdded a `variant` prop to `Button`.\n";
    let intent =
        parse_change_intent(content, "brave-pandas-smile", Path::new("/x/brave-pandas-smile.md"))
            .expect("intent parses");
    assert_eq!(
        intent.releases,
        IndexMap::from([
            ("@example/ui".to_string(), IntentBumpType::Minor),
            ("@example/core".to_string(), IntentBumpType::Patch),
        ]),
    );
    assert_eq!(intent.summary, "Added a `variant` prop to `Button`.");
}

#[test]
fn tolerates_a_utf8_bom_and_crlf_line_endings() {
    let content = "\u{FEFF}---\r\nfoo: patch\r\n---\r\n\r\nA fix.\r\n";
    let intent = parse_change_intent(content, "id", Path::new("/x/id.md")).expect("intent parses");
    assert_eq!(intent.releases, IndexMap::from([("foo".to_string(), IntentBumpType::Patch)]));
    assert_eq!(intent.summary, "A fix.");
}

#[test]
fn rejects_an_invalid_bump_type() {
    let err = parse_change_intent("---\nfoo: gigantic\n---\nx", "id", Path::new("/x/id.md"))
        .expect_err("parse must fail");
    assert!(err.to_string().contains("invalid bump type"), "unexpected error: {err}");
}

#[test]
fn rejects_a_file_without_frontmatter() {
    let err = parse_change_intent("Just some text.", "id", Path::new("/x/id.md"))
        .expect_err("parse must fail");
    assert!(err.to_string().contains("no YAML frontmatter"), "unexpected error: {err}");
}

#[test]
fn write_change_intent_output_round_trips_through_read_change_intents() {
    let workspace = tempfile::tempdir().expect("create temp workspace");
    let releases = IndexMap::from([("@example/ui".to_string(), IntentBumpType::Minor)]);
    let id =
        write_change_intent(workspace.path(), &releases, "Added a thing.").expect("intent writes");
    let intents = read_change_intents(workspace.path()).expect("intents read");
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].id, id);
    assert_eq!(intents[0].releases, releases);
    assert_eq!(intents[0].summary, "Added a thing.");
}
