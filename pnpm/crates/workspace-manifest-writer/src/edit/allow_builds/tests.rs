use super::allow_build_key_package_name;

#[test]
fn extracts_package_name_across_ascii_scoped_and_utf8_keys() {
    assert_eq!(allow_build_key_package_name("foo"), Some("foo"));
    assert_eq!(allow_build_key_package_name("foo@1.0.0"), Some("foo"));
    assert_eq!(allow_build_key_package_name("@scope/foo@1.0.0"), Some("@scope/foo"));
    assert_eq!(allow_build_key_package_name("ä@1.0.0"), Some("ä"));
    assert_eq!(allow_build_key_package_name("ä@https://example.invalid/a.tgz"), Some("ä"));
    assert_eq!(allow_build_key_package_name("@ä/ü@1.0.0"), Some("@ä/ü"));
    assert_eq!(allow_build_key_package_name("ä@"), None);
    assert_eq!(allow_build_key_package_name("@"), Some("@"));
    assert_eq!(allow_build_key_package_name(""), None);
}
