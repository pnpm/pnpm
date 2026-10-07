use super::PackageSpec;

#[test]
fn parse_spec_bare_name() {
    assert_eq!(PackageSpec::parse("foo"), Some(PackageSpec { name: "foo".into(), version: None }));
}

#[test]
fn parse_spec_name_with_version() {
    assert_eq!(
        PackageSpec::parse("foo@1.0.0"),
        Some(PackageSpec { name: "foo".into(), version: Some("1.0.0".into()) }),
    );
}

#[test]
fn parse_spec_scoped_package() {
    assert_eq!(
        PackageSpec::parse("@scope/foo"),
        Some(PackageSpec { name: "@scope/foo".into(), version: None }),
    );
}

#[test]
fn parse_spec_scoped_with_version() {
    assert_eq!(
        PackageSpec::parse("@scope/foo@1.0.0"),
        Some(PackageSpec { name: "@scope/foo".into(), version: Some("1.0.0".into()) }),
    );
}

#[test]
fn parse_spec_scoped_with_tag() {
    assert_eq!(
        PackageSpec::parse("@scope/foo@latest"),
        Some(PackageSpec { name: "@scope/foo".into(), version: Some("latest".into()) }),
    );
}

#[test]
fn parse_spec_trims_whitespace() {
    assert_eq!(
        PackageSpec::parse("  foo  "),
        Some(PackageSpec { name: "foo".into(), version: None }),
    );
}

#[test]
fn parse_spec_trailing_at_is_empty_version() {
    assert_eq!(PackageSpec::parse("foo@"), Some(PackageSpec { name: "foo".into(), version: None }));
}

#[test]
fn parse_spec_invalid_name_returns_none() {
    assert_eq!(PackageSpec::parse(""), None);
    assert_eq!(PackageSpec::parse("   "), None);
    assert_eq!(PackageSpec::parse("not a valid name"), None);
}
