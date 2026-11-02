use super::{MAX_NOTE_BYTES, note_digest, validate_note};

#[test]
fn content_rules() {
    assert!(validate_note("## 1.0.0\n\n- a\tb\n").is_ok());
    for refused in ["a\r\nb", "\u{1b}[31mred", "safe\u{202E}txt", "\u{2067}x", "a\u{7}"] {
        assert!(validate_note(refused).is_err(), "{refused:?}");
    }
    assert!(validate_note(&"x".repeat(MAX_NOTE_BYTES)).is_ok());
    assert!(validate_note(&"x".repeat(MAX_NOTE_BYTES + 1)).is_err());
    assert_eq!(note_digest(""), "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=");
}
