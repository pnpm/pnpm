use super::{extract_env_document, extract_main_document};

#[test]
fn returns_entire_content_when_it_does_not_start_with_separator() {
    let content = "lockfileVersion: 9.0\npackages: {}\n";
    assert_eq!(extract_main_document(content), content);
}

#[test]
fn returns_empty_string_when_content_starts_with_separator_but_has_no_second_separator() {
    let content = "---\nfoo: bar\n";
    assert_eq!(extract_main_document(content), "");
}

#[test]
fn returns_the_second_document_from_a_combined_file() {
    let main = "lockfileVersion: 9.0\npackages: {}\n";
    let combined = format!("---\nfoo: bar\n---\n{main}");
    assert_eq!(extract_main_document(&combined), main);
}

#[test]
fn splits_a_crlf_combined_file() {
    let combined = "---\r\nfoo: bar\r\n---\r\nlockfileVersion: 9.0\r\npackages: {}\r\n";
    assert_eq!(extract_main_document(combined), "lockfileVersion: 9.0\npackages: {}\n");
    assert_eq!(extract_env_document(combined).as_deref(), Some("foo: bar"));
}

#[test]
fn splits_a_combined_file_behind_a_byte_order_mark() {
    let combined = "\u{feff}---\nfoo: bar\n---\nlockfileVersion: 9.0\n";
    assert_eq!(extract_main_document(combined), "lockfileVersion: 9.0\n");
    assert_eq!(extract_env_document(combined).as_deref(), Some("foo: bar"));
}

#[test]
fn normalizes_a_single_document_file() {
    let content = "\u{feff}lockfileVersion: 9.0\r\npackages: {}\r\n";
    assert_eq!(extract_main_document(content), "lockfileVersion: 9.0\npackages: {}\n");
    assert_eq!(extract_env_document(content), None);
}
