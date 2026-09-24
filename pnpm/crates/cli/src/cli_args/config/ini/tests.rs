use super::IniDocument;
use pretty_assertions::assert_eq;

#[test]
fn parse_and_serialize_preserves_comments_and_blank_lines() {
    let input = "# Comment 1\n; Comment 2\n\nkey1=value1\n# Comment 3\nkey2=value2\n";
    let doc = IniDocument::parse(input);
    assert_eq!(doc.serialize(), input);
}

#[test]
fn parse_and_serialize_preserves_crlf() {
    let input = "# Comment 1\r\nkey1=val1\r\n";
    let doc = IniDocument::parse(input);
    assert_eq!(doc.serialize(), input);
}

#[test]
fn parse_and_serialize_preserves_bom() {
    let input = "\u{feff}# Comment\nkey=val\n";
    let doc = IniDocument::parse(input);
    assert_eq!(doc.serialize(), input);
}

#[test]
fn repeated_keys_preserved_on_unrelated_set() {
    let input = "# CAs\nca=certA\nca=certB\n\nregistry=https://old/\n";
    let mut doc = IniDocument::parse(input);
    doc.set("registry", &["https://new/".to_string()]);
    let expected = "# CAs\nca=certA\nca=certB\n\nregistry=https://new/\n";
    assert_eq!(doc.serialize(), expected);
    assert_eq!(doc.get_all("ca"), vec!["certA", "certB"]);
}

#[test]
fn set_replaces_in_place_and_removes_duplicate_occurrences() {
    let input = "ca=certA\nca=certB\nregistry=https://reg/\n";
    let mut doc = IniDocument::parse(input);
    doc.set("ca", &["certC".to_string()]);
    let expected = "ca=certC\nregistry=https://reg/\n";
    assert_eq!(doc.serialize(), expected);
    assert_eq!(doc.get("ca"), Some("certC"));
}

#[test]
fn set_array_replaces_in_place_and_removes_duplicate_occurrences() {
    let input = "ca=certOld1\nca=certOld2\nother=val\n";
    let mut doc = IniDocument::parse(input);
    doc.set("ca", &["certNew1".to_string(), "certNew2".to_string()]);
    let expected = "ca=certNew1\nca=certNew2\nother=val\n";
    assert_eq!(doc.serialize(), expected);
    assert_eq!(doc.get_all("ca"), vec!["certNew1", "certNew2"]);
}

#[test]
fn set_new_key_appends_at_end() {
    let input = "# comment\nca=certA\n";
    let mut doc = IniDocument::parse(input);
    doc.set("registry", &["https://new/".to_string()]);
    let expected = "# comment\nca=certA\nregistry=https://new/\n";
    assert_eq!(doc.serialize(), expected);
}

#[test]
fn delete_removes_only_specified_key_preserving_repeated_keys_and_comments() {
    let input = "# comment\nca=certA\nca=certB\nregistry=https://reg/\n";
    let mut doc = IniDocument::parse(input);
    assert!(doc.delete("registry"));
    let expected = "# comment\nca=certA\nca=certB\n";
    assert_eq!(doc.serialize(), expected);
    assert_eq!(doc.get_all("ca"), vec!["certA", "certB"]);
}

#[test]
fn delete_repeated_key_removes_all_its_occurrences() {
    let input = "# comment\nca=certA\nca=certB\nregistry=https://reg/\n";
    let mut doc = IniDocument::parse(input);
    assert!(doc.delete("ca"));
    let expected = "# comment\nregistry=https://reg/\n";
    assert_eq!(doc.serialize(), expected);
    assert_eq!(doc.get_all("ca"), Vec::<&str>::new());
}

#[test]
fn delete_nonexistent_key_returns_false_and_preserves_document() {
    let input = "# comment\nca=certA\n";
    let mut doc = IniDocument::parse(input);
    assert!(!doc.delete("registry"));
    assert_eq!(doc.serialize(), input);
}

#[test]
fn entries_iterates_in_document_order() {
    let input = "ca=certA\nca=certB\nregistry=https://reg/\n";
    let doc = IniDocument::parse(input);
    let entries: Vec<(&str, &str)> = doc.entries().collect();
    assert_eq!(entries, vec![("ca", "certA"), ("ca", "certB"), ("registry", "https://reg/")]);
}

#[test]
fn parse_and_serialize_preserves_mixed_line_endings() {
    let input = "key1=val1\r\n# comment\nkey2=val2\r\nkey3=val3";
    let doc = IniDocument::parse(input);
    assert_eq!(doc.serialize(), input);

    let mut modified = doc;
    modified.set("key2", &["val2_updated".to_string()]);
    let expected = "key1=val1\r\n# comment\nkey2=val2_updated\r\nkey3=val3";
    assert_eq!(modified.serialize(), expected);
}

#[test]
fn parse_and_serialize_preserves_file_without_trailing_newline() {
    let input = "# comment\nkey1=val1\nkey2=val2";
    let doc = IniDocument::parse(input);
    assert_eq!(doc.serialize(), input);
}

#[test]
fn set_existing_key_preserves_file_without_trailing_newline() {
    let input = "key1=val1\nkey2=val2";
    let mut doc = IniDocument::parse(input);
    doc.set("key2", &["val2_updated".to_string()]);
    let expected = "key1=val1\nkey2=val2_updated";
    assert_eq!(doc.serialize(), expected);

    doc.set("key1", &["val1_updated".to_string()]);
    let expected2 = "key1=val1_updated\nkey2=val2_updated";
    assert_eq!(doc.serialize(), expected2);
}

#[test]
fn set_new_key_on_file_without_trailing_newline_appends_terminator() {
    let input = "key1=val1\nkey2=val2";
    let mut doc = IniDocument::parse(input);
    doc.set("key3", &["val3".to_string()]);
    let expected = "key1=val1\nkey2=val2\nkey3=val3\n";
    assert_eq!(doc.serialize(), expected);
}

#[test]
fn delete_key_preserves_file_without_trailing_newline() {
    let input = "key1=val1\nkey2=val2";
    let mut doc = IniDocument::parse(input);
    assert!(doc.delete("key1"));
    assert_eq!(doc.serialize(), "key2=val2");
}
