use super::{ParsedKey, parse_key};
use pretty_assertions::assert_eq;

#[test]
fn bare_name_returns_empty() {
    assert_eq!(parse_key("lodash"), ParsedKey::default());
}

#[test]
fn bare_scoped_name_returns_empty() {
    // `@scope/foo` has `@` at index 0; the search for a version
    // separator starts at index 1 and finds no second `@`. Falls
    // into the wildcard bucket.
    assert_eq!(parse_key("@scope/foo"), ParsedKey::default());
}

#[test]
fn name_at_exact_version() {
    assert_eq!(
        parse_key("lodash@4.17.21"),
        ParsedKey { name: Some("lodash"), version: Some("4.17.21"), non_semver_version: None },
    );
}

#[test]
fn scoped_name_at_exact_version() {
    assert_eq!(
        parse_key("@scope/foo@1.0.0"),
        ParsedKey { name: Some("@scope/foo"), version: Some("1.0.0"), non_semver_version: None },
    );
}

#[test]
fn name_at_range_becomes_non_semver() {
    assert_eq!(
        parse_key("lodash@^4.17.0"),
        ParsedKey { name: Some("lodash"), version: None, non_semver_version: Some("^4.17.0") },
    );
}

/// An empty version after the `@` separator drops back to the empty
/// result.
#[test]
fn empty_version_returns_empty() {
    assert_eq!(parse_key("lodash@"), ParsedKey::default());
}

/// `1.x.x` is a valid semver *range* but not a valid semver
/// *version*, so the parsed result lands in `non_semver_version`.
#[test]
fn version_with_x_is_range() {
    assert_eq!(
        parse_key("foo@1.x.x"),
        ParsedKey { name: Some("foo"), version: None, non_semver_version: Some("1.x.x") },
    );
}
