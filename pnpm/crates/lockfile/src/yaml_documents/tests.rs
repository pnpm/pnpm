use super::extract_main_document;

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
