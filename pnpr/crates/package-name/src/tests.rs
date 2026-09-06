use super::{CanonicalPackageName, Ecosystem, is_safe_path_segment};

#[test]
fn canonicalizes_each_ecosystem() {
    assert_eq!(CanonicalPackageName::parse("React", Ecosystem::Npm).unwrap().as_str(), "React");
    assert_eq!(
        CanonicalPackageName::parse("Serde_JSON", Ecosystem::Cargo).unwrap().as_str(),
        "serde_json",
    );
    assert_eq!(
        CanonicalPackageName::parse("Demo.Package_name", Ecosystem::Pypi).unwrap().as_str(),
        "demo-package-name",
    );
}

#[test]
fn reports_ecosystem_specific_name_errors() {
    let cargo_error = CanonicalPackageName::parse("9lives", Ecosystem::Cargo).unwrap_err();
    assert_eq!(
        cargo_error.public_message(),
        r#"Package name "9lives" is not valid for cargo: crate name "9lives" must start with a letter or `_`"#,
    );

    let python_error = CanonicalPackageName::parse("demo package", Ecosystem::Pypi).unwrap_err();
    assert_eq!(
        python_error.public_message(),
        r#"Package name "demo package" is not valid for pypi: "demo package" is not a valid Python project name"#,
    );
}

#[test]
fn accepts_unscoped() {
    let name = CanonicalPackageName::parse("lodash", Ecosystem::Npm).unwrap();
    assert_eq!(name.as_str(), "lodash");
    assert_eq!(name.tarball_name_for_version("4.17.21"), "lodash-4.17.21.tgz");
    name.parse_tarball_name("lodash-4.17.21.tgz").unwrap();
}

#[test]
fn accepts_scoped() {
    let name = CanonicalPackageName::parse("@types/node", Ecosystem::Npm).unwrap();
    assert_eq!(name.as_str(), "@types/node");
    assert_eq!(name.tarball_name_for_version("20.0.0"), "node-20.0.0.tgz");
    name.parse_tarball_name("node-20.0.0.tgz").unwrap();
}

#[test]
fn rejects_traversal() {
    assert!(CanonicalPackageName::parse("..", Ecosystem::Npm).is_err());
    assert!(CanonicalPackageName::parse("foo/../bar", Ecosystem::Npm).is_err());
    assert!(CanonicalPackageName::parse("@scope/..", Ecosystem::Npm).is_err());
}

#[test]
fn rejects_dot_prefix() {
    assert!(CanonicalPackageName::parse(".hidden", Ecosystem::Npm).is_err());
}

#[test]
fn rejects_tarball_for_other_package() {
    let name = CanonicalPackageName::parse("foo", Ecosystem::Npm).unwrap();
    assert!(name.parse_tarball_name("bar-1.0.0.tgz").is_err());
    assert!(name.parse_tarball_name("../foo-1.0.0.tgz").is_err());
    assert!(name.parse_tarball_name("foo-1.0.0").is_err());
}

/// `C:foo` is a drive-relative prefix on Windows — `PathBuf::join` replaces
/// the base path with it instead of descending — so a `:` anywhere in a name,
/// version, or preserved non-canonical tarball basename must be rejected
/// before it can become a storage or cache path segment.
#[test]
fn rejects_windows_drive_prefixes() {
    assert!(!is_safe_path_segment("C:evil.tgz"));
    assert!(!is_safe_path_segment("c:"));
    assert!(CanonicalPackageName::parse("C:foo", Ecosystem::Npm).is_err());
    assert!(CanonicalPackageName::parse("@scope/C:foo", Ecosystem::Npm).is_err());
    let name = CanonicalPackageName::parse("foo", Ecosystem::Npm).unwrap();
    assert!(name.parse_tarball_name("foo-1.0.0:x.tgz").is_err());
}

/// A name is interpolated into an upstream URL, so a `?`, `#`, or `%` in it
/// would address a different package there than the one it is authorized and
/// cached under.
#[test]
fn rejects_url_delimiters_and_blanks() {
    for raw in ["foo?bar", "foo#bar", "foo%2fbar", "foo bar", "foo\tbar", "foo\u{7f}bar"] {
        assert!(CanonicalPackageName::parse(raw, Ecosystem::Npm).is_err(), "{raw:?}");
        assert!(!is_safe_path_segment(raw), "{raw:?}");
    }
    assert!(CanonicalPackageName::parse("@scope/foo?bar", Ecosystem::Npm).is_err());
    assert!(CanonicalPackageName::parse("@sco?pe/foo", Ecosystem::Npm).is_err());
    let name = CanonicalPackageName::parse("foo", Ecosystem::Npm).unwrap();
    assert!(name.parse_tarball_name("foo-1.0.0?x.tgz").is_err());
}
