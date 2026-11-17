use node_semver::Version;

use super::{is_any_version_range, is_valid_semver_range, parse_strict_semver_range};

#[test]
fn empty_and_blank_ranges_are_any_version() {
    assert!(is_any_version_range(""));
    assert!(is_any_version_range(" "));
    assert!(is_any_version_range("   "));
    assert!(is_any_version_range("\t"));
}

#[test]
fn a_union_with_an_empty_member_is_any_version() {
    assert!(is_any_version_range("||"));
    assert!(is_any_version_range("^1.0.0 || "));
    assert!(is_any_version_range("|| ^1.0.0"));
    assert!(is_any_version_range("^1.0.0||"));
}

#[test]
fn an_empty_member_widens_a_union_whose_siblings_do_not_parse() {
    assert!(is_any_version_range("latest || "));
    assert!(is_any_version_range("|| latest"));
    assert!(is_any_version_range("garbage ||"));
}

#[test]
fn bounded_ranges_are_not_any_version() {
    assert!(!is_any_version_range("^1.0.0"));
    assert!(!is_any_version_range("1.0.0"));
    assert!(!is_any_version_range(">=1.0.0 <2.0.0"));
    assert!(!is_any_version_range("^1.0.0 || ^2.0.0"));
    assert!(!is_any_version_range("latest"));
}

#[test]
fn any_version_spellings_are_valid_ranges() {
    assert!(is_valid_semver_range(""));
    assert!(is_valid_semver_range(" "));
    assert!(is_valid_semver_range("||"));
    assert!(is_valid_semver_range("^1.0.0 || "));
}

#[test]
fn ordinary_ranges_stay_valid_and_tags_stay_invalid() {
    for valid in [
        "*",
        "x",
        "X",
        "1.x",
        "1.2.x",
        "*.*",
        "^1.0.0",
        "^ 1.0.0",
        "~1.0.0",
        "~> 1.0.0",
        "~ > 1.0.0",
        "=1.0.0",
        "= 1.0.0",
        "=v1.0.0",
        "v1.0.0",
        ">=1.0.0 <2.0.0",
        ">= 1.0.0 <= 2.0.0",
        "> =1.0.0",
        "< =2.0.0",
        "1.0.0 - 2.0.0",
        "1.0 - 2.0",
        "v1.0.0 - v2.0.0",
        "^1.0.0 || ^2.0.0",
        "1.2.3-alpha.1+build.2",
        "1.2+build",
    ] {
        assert!(is_valid_semver_range(valid), "expected valid: {valid:?}");
    }
    assert!(!is_valid_semver_range("latest"));
    assert!(!is_valid_semver_range("npm:bar@^5"));
}

#[test]
fn a_union_with_an_unparsable_member_is_not_a_valid_range() {
    assert!(!is_valid_semver_range("latest || "));
    assert!(!is_valid_semver_range("|| latest"));
    assert!(!is_valid_semver_range("garbage ||"));
    assert!(!is_valid_semver_range("npm:bar@^5 || "));
}

#[test]
fn a_comparator_set_with_an_unparsable_token_is_not_a_valid_range() {
    for invalid in [
        "bar@>=7.0.0 <8.0.0",
        "@scope/pkg@>=1.0.0 <2.0.0",
        "bar@6.0.0 - 8.0.0",
        "npm:bar@>=7.0.0 <8.0.0",
        "npm:bar@6.0.0 - 8.0.0",
        "foo >=1.0.0",
        ">=1.0.0 foo",
        "1.0.0 - 2.0.0 <3.0.0",
        "0.1.0 1.0.0 - 2.0.0",
        " - 2.0.0",
        "1.0.0 -",
        "01.02.03",
        "1.0.0alpha",
        "v 1.0.0",
        "> = 1.0.0",
        "< = 2.0.0",
        "1.x.0",
        "x.1.0",
        "1.2-alpha",
        "1.0.0-01",
    ] {
        assert!(!is_valid_semver_range(invalid), "expected invalid: {invalid:?}");
        assert!(parse_strict_semver_range(invalid).is_none(), "expected None: {invalid:?}");
    }
}

#[test]
fn parse_strict_semver_range_handles_any_and_wildcard_operators() {
    let v1 = Version::parse("1.2.3").unwrap();
    let v2 = Version::parse("2.0.0").unwrap();
    for unbounded in ["", " ", "^1.0.0 || ", "^x", "~x", "~*", "=*", "~>x"] {
        let parsed = parse_strict_semver_range(unbounded)
            .unwrap_or_else(|| panic!("expected Some for {unbounded:?}"));
        assert!(parsed.satisfies(&v1), "for {unbounded:?}");
        assert!(parsed.satisfies(&v2), "for {unbounded:?}");
    }
    let tilde_wildcard = parse_strict_semver_range("~1.x.0").unwrap();
    assert!(tilde_wildcard.satisfies(&v1));
    assert!(!tilde_wildcard.satisfies(&v2));
}
