use super::split_name_spec;

#[test]
fn splits_ascii_scoped_and_utf8_package_specs() {
    assert_eq!(split_name_spec("foo"), ("foo", None));
    assert_eq!(split_name_spec("foo@1.0.0"), ("foo", Some("1.0.0")));
    assert_eq!(split_name_spec("@scope/foo"), ("@scope/foo", None));
    assert_eq!(split_name_spec("@scope/foo@1.0.0"), ("@scope/foo", Some("1.0.0")));
    assert_eq!(split_name_spec("ä"), ("ä", None));
    assert_eq!(split_name_spec("ä@1.0.0"), ("ä", Some("1.0.0")));
    assert_eq!(split_name_spec("@ä/ü@^2.0.0"), ("@ä/ü", Some("^2.0.0")));
}
