use super::version_separator_index;

#[test]
fn finds_separator_for_ascii_and_scoped_names() {
    assert_eq!(version_separator_index(""), None);
    assert_eq!(version_separator_index("@"), None);
    assert_eq!(version_separator_index("@@"), Some(1));
    assert_eq!(version_separator_index("foo"), None);
    assert_eq!(version_separator_index("@scope/foo"), None);
    assert_eq!(version_separator_index("foo@1.0.0"), Some(3));
    assert_eq!(version_separator_index("@scope/foo@1.0.0"), Some(10));
    assert_eq!(version_separator_index("foo@git+ssh://git@github.com/org/foo.git"), Some(3));
}

#[test]
fn handles_multibyte_utf8_package_names() {
    assert_eq!(version_separator_index("ä"), None);
    assert_eq!(version_separator_index("ä@1.0.0"), Some("ä".len()));
    assert_eq!(version_separator_index("@ä/ü@1.0.0"), Some("@ä/ü".len()));
    assert_eq!(version_separator_index("😀@https://example.invalid/a.tgz"), Some("😀".len()));
}
