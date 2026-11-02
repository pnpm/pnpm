use super::{MAX_NOTE_BYTES, NoteContentError, normalize_line_endings, validate_note};
use pretty_assertions::assert_eq;

#[test]
fn normalizes_crlf_and_lone_cr_to_lf() {
    assert_eq!(normalize_line_endings("one\r\ntwo\rthree\n"), "one\ntwo\nthree\n");
}

#[test]
fn accepts_markdown_with_tabs_and_line_feeds() {
    validate_note("## Changes\n\n- one\n\t- nested\n").expect("note is valid");
}

#[test]
fn rejects_a_note_over_the_size_limit() {
    let note = "a".repeat(MAX_NOTE_BYTES + 1);
    let error = validate_note(&note).expect_err("note is too large");
    assert!(matches!(error, NoteContentError::TooLarge { size } if size == MAX_NOTE_BYTES + 1));
}

#[test]
fn accepts_a_note_at_the_size_limit() {
    validate_note(&"a".repeat(MAX_NOTE_BYTES)).expect("note fits");
}

#[test]
fn rejects_control_characters_with_their_line() {
    let error = validate_note("fine\nclear \u{1b}[2J screen").expect_err("escape is rejected");
    assert!(matches!(error, NoteContentError::ForbiddenCharacter { code_point: 0x1b, line: 2 },));
}

#[test]
fn rejects_carriage_returns() {
    let error = validate_note("one\r\ntwo").expect_err("carriage return is rejected");
    assert!(matches!(error, NoteContentError::ForbiddenCharacter { code_point: 0x0d, line: 1 }));
}

#[test]
fn rejects_bidirectional_overrides() {
    for override_char in ['\u{202E}', '\u{2066}', '\u{2069}'] {
        let note = format!("safe{override_char}text");
        let error = validate_note(&note).expect_err("override is rejected");
        assert!(matches!(error, NoteContentError::ForbiddenCharacter { line: 1, .. }));
    }
}
