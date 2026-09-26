use super::stringify;
use serde_json::{Value, json};

#[test]
fn pretty_and_compact_forms_match_json5_stringify() {
    let value = json!({"name": "j5", "version": "1.0.1"});
    assert_eq!(stringify(&value, "  "), "{\n  name: 'j5',\n  version: '1.0.1',\n}");
    assert_eq!(stringify(&value, ""), "{name:'j5',version:'1.0.1'}");
    assert_eq!(stringify(&value, "\t"), "{\n\tname: 'j5',\n\tversion: '1.0.1',\n}");
}

#[test]
fn quotes_keys_that_are_not_identifiers_and_keeps_identifier_keys_bare() {
    let value = json!({
        "dependencies": {"@scope/pkg": "1.0.0", "is-odd": "1.0.0"},
        "$schema": "https://example.test/schema.json",
    });
    assert_eq!(
        stringify(&value, "  "),
        "{\n  dependencies: {\n    '@scope/pkg': '1.0.0',\n    'is-odd': '1.0.0',\n  },\n  $schema: 'https://example.test/schema.json',\n}",
    );
}

#[test]
fn prefers_the_quote_that_needs_less_escaping() {
    assert_eq!(stringify(&json!("it's"), ""), r#""it's""#);
    assert_eq!(stringify(&json!(r#"say "hi""#), ""), r#"'say "hi"'"#);
    assert_eq!(stringify(&json!(r#"a'b"c"#), ""), r#"'a\'b"c'"#);
    assert_eq!(stringify(&json!(""), ""), "''");
}

#[test]
fn escapes_controls_and_round_trips_through_json5() {
    let value = json!({
        "name": "j5",
        "note": "a\nb\t\0c\u{1}\u{2028}\u{2029}",
        "zero": "\u{0}1",
        "flags": [true, false, null],
        "nested": {"ok": true},
        "emptyObject": {},
        "emptyArray": [],
        "n": 1,
        "f": 1.5,
    });
    let text = stringify(&value, "  ");
    assert_eq!(json5::from_str::<Value>(&text).unwrap(), value);
    assert!(text.contains(r"\n"));
    assert!(text.contains(r"\u2028"));
    assert!(text.contains(r"\x00"));
    assert!(text.contains(r"\x01"));
}

#[test]
fn caps_the_indent_unit_at_ten_characters() {
    let text = stringify(&json!({"name": "j5"}), &" ".repeat(12));
    assert_eq!(text, format!("{{\n{}name: 'j5',\n}}", " ".repeat(10)));
}
