use super::parse_catalog_protocol;

#[test]
fn parses_named_catalog() {
    assert_eq!(parse_catalog_protocol("catalog:foo"), Some("foo"));
    assert_eq!(parse_catalog_protocol("catalog:bar"), Some("bar"));
}

#[test]
fn returns_none_for_specifier_not_using_catalog_protocol() {
    assert_eq!(parse_catalog_protocol("^1.0.0"), None);
}

#[test]
fn parses_explicit_default_catalog() {
    assert_eq!(parse_catalog_protocol("catalog:default"), Some("default"));
}

#[test]
fn parses_implicit_default_catalog() {
    assert_eq!(parse_catalog_protocol("catalog:"), Some("default"));
}
