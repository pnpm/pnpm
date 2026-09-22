use super::{RELOCATION_MARKER, extract_comments, restore_comments};
use serde_json::{Value, json};

fn assert_restored(original: &str, expected: &Value, comments: &[&str]) -> String {
    json5::from_str::<Value>(original).unwrap();
    let serialized = serde_json::to_string_pretty(expected).unwrap();
    let restored = restore_comments(original, &serialized);
    eprintln!("RESTORED:\n{restored}");
    assert_eq!(json5::from_str::<Value>(&restored).unwrap(), *expected);
    let (_, extracted) = extract_comments(&restored);
    for comment in comments {
        assert_eq!(
            extracted
                .iter()
                .filter(|item| item.text == *comment)
                .count(),
            1
        );
    }
    restored
}

#[test]
fn preserves_comments_when_version_changes() {
    let original = "// package\n{\n  name: 'demo',\n  // release\n  version: '1.0.0', // version note\n}\n// footer";
    assert_restored(
        original,
        &json!({"name": "demo", "version": "2.0.0"}),
        &["// package", "// release", "// version note", "// footer"],
    );
}

#[test]
fn keeps_dependency_comments_after_removal_and_sorting() {
    let original = "{\n  dependencies: {\n    z: '1', // z note\n    removed: '1', // removed note\n    a: '1', // a note\n  },\n}";
    assert_restored(
        original,
        &json!({"dependencies": {"a": "1", "z": "1"}}),
        &["// z note", "// removed note", "// a note"],
    );
}

#[test]
fn marks_comments_whose_surrounding_lines_were_removed() {
    let restored = assert_restored(
        "{\nremoved: 1,\n// retained note\nalsoRemoved: 2,\nname: 'demo'\n}",
        &json!({"name": "demo"}),
        &["// retained note"],
    );
    assert!(restored.contains("[comment possibly relocated by pnpm]"));
}

#[test]
fn skips_comment_markers_inside_strings_and_escaped_quotes() {
    let original = r#"{url: 'https://example.com', text: 'it\'s /* not a comment */', other: "quote: \" // text"} // actual"#;
    assert_restored(
        original,
        &json!({"url": "https://example.com", "text": "it's /* not a comment */", "other": "quote: \" // text"}),
        &["// actual"],
    );
    assert_eq!(extract_comments(original).1.len(), 1);
}

#[test]
fn skips_escaped_line_breaks_inside_strings() {
    let original = "{value: 'a\\\r\n//still a string\\\u{2028}/*also a string*/'} // actual";
    assert_restored(
        original,
        &json!({"value": "a//still a string/*also a string*/"}),
        &["// actual"],
    );
    assert_eq!(extract_comments(original).1.len(), 1);
}

#[test]
fn separates_line_comments_anchored_to_the_same_line() {
    let original = "{\n// first\n// second\nname: 'demo'\n}";
    let serialized = r#"{"name":"demo"}"#;
    let restored = restore_comments(original, serialized);
    eprintln!("RESTORED:\n{restored}");
    assert_eq!(json5::from_str::<Value>(&restored).unwrap(), json!({"name": "demo"}));
    let (_, comments) = extract_comments(&restored);
    assert_eq!(
        comments
            .iter()
            .filter(|comment| comment.text == "// first")
            .count(),
        1
    );
    assert_eq!(
        comments
            .iter()
            .filter(|comment| comment.text == "// second")
            .count(),
        1
    );
}

#[test]
fn preserves_compact_block_comments_and_eof_comment() {
    assert_restored(
        "{/*first*/name:/*second*/'demo'/*third*/} // eof",
        &json!({"name": "demo"}),
        &["/*first*/", "/*second*/", "/*third*/", "// eof"],
    );
}

#[test]
fn recognizes_unicode_comment_line_terminators() {
    assert_restored(
        "{// first\u{2028}// second\u{2029}name: 'demo'}",
        &json!({"name": "demo"}),
        &["// first", "// second"],
    );
}

#[test]
fn preserves_multiline_block_comment() {
    assert_restored(
        "{\n/* first\n * second\n */\nname: 'demo'\n}",
        &json!({"name": "demo"}),
        &["/* first\n * second\n */"],
    );
}

#[test]
fn leaves_comment_free_serialization_unchanged() {
    assert_eq!(
        restore_comments("{name: 'demo'}", "{\"name\":\"demo\"}\n"),
        "{\"name\":\"demo\"}\n"
    );
}

#[test]
fn repeated_edits_do_not_accumulate_relocation_markers() {
    let mut source = "{\nremoved: 1,\n// retained note\nvalue: 1\n}".to_owned();
    for version in 2..=11 {
        source = assert_restored(&source, &json!({"value": version}), &["// retained note"]);
        let (_, comments) = extract_comments(&source);
        assert_eq!(
            comments
                .iter()
                .filter(|comment| comment.text == RELOCATION_MARKER)
                .count(),
            1
        );
    }
}

#[test]
fn marker_inside_string_does_not_suppress_relocation_annotation() {
    let source = "{\nremoved: 1,\n// retained note\nvalue: 1,\ntext: '/* [comment possibly relocated by pnpm] */'\n}";
    assert_restored(
        source,
        &json!({"value": 2, "text": RELOCATION_MARKER}),
        &["// retained note", RELOCATION_MARKER],
    );
}
