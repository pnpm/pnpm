use super::{PackageName, is_safe_path_segment};

#[test]
fn accepts_unscoped() {
    let name = PackageName::parse("lodash").unwrap();
    assert_eq!(name.as_str(), "lodash");
    assert_eq!(name.tarball_name_for_version("4.17.21"), "lodash-4.17.21.tgz");
    name.parse_tarball_name("lodash-4.17.21.tgz").unwrap();
}

#[test]
fn accepts_scoped() {
    let name = PackageName::parse("@types/node").unwrap();
    assert_eq!(name.as_str(), "@types/node");
    assert_eq!(name.tarball_name_for_version("20.0.0"), "node-20.0.0.tgz");
    name.parse_tarball_name("node-20.0.0.tgz").unwrap();
}

#[test]
fn rejects_traversal() {
    assert!(PackageName::parse("..").is_err());
    assert!(PackageName::parse("foo/../bar").is_err());
    assert!(PackageName::parse("@scope/..").is_err());
}

#[test]
fn rejects_dot_prefix() {
    assert!(PackageName::parse(".hidden").is_err());
}

#[test]
fn rejects_tarball_for_other_package() {
    let name = PackageName::parse("foo").unwrap();
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
    assert!(PackageName::parse("C:foo").is_err());
    assert!(PackageName::parse("@scope/C:foo").is_err());
    let name = PackageName::parse("foo").unwrap();
    assert!(name.parse_tarball_name("foo-1.0.0:x.tgz").is_err());
}

/// A name is interpolated into an upstream URL, so a `?`, `#`, or `%` in it
/// would address a different package there than the one it is authorized and
/// cached under.
#[test]
fn rejects_url_delimiters_and_blanks() {
    for raw in ["foo?bar", "foo#bar", "foo%2fbar", "foo bar", "foo\tbar", "foo\u{7f}bar"] {
        assert!(PackageName::parse(raw).is_err(), "{raw:?}");
        assert!(!is_safe_path_segment(raw), "{raw:?}");
    }
    assert!(PackageName::parse("@scope/foo?bar").is_err());
    assert!(PackageName::parse("@sco?pe/foo").is_err());
    let name = PackageName::parse("foo").unwrap();
    assert!(name.parse_tarball_name("foo-1.0.0?x.tgz").is_err());
}
