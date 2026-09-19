use super::{sanitize, sanitize_inline};
use std::borrow::Cow;

#[test]
fn strips_format_characters() {
    let text = "safe\u{00AD}\u{202E}\u{2066}\u{E0020}text";

    assert_eq!(sanitize(text), "safetext");
    assert_eq!(sanitize_inline(text), "safetext");
}

#[test]
fn preserves_multiline_whitespace_only_outside_inline_fields() {
    let text = "safe\n\ttext";
    let sanitized = sanitize(text);

    eprintln!("actual: {sanitized:?}\nexpected: {text:?}");
    assert_eq!(sanitized, text);
    assert_eq!(sanitize_inline(text), "safetext");
}

#[test]
fn borrows_text_that_does_not_need_sanitizing() {
    assert!(matches!(sanitize("safe text"), Cow::Borrowed(_)));
    assert!(matches!(sanitize_inline("safe text"), Cow::Borrowed(_)));
}

/// U+2028 and U+2029 end a line for a JSON or JavaScript consumer without
/// being control characters, so a name carrying one could split a rendered
/// warning into a second, attacker-shaped line.
#[test]
fn strips_unicode_line_and_paragraph_separators() {
    let text = "safe\u{2028}\u{2029}text";

    assert_eq!(sanitize(text), "safetext");
    assert_eq!(sanitize_inline(text), "safetext");
}
