use super::{is_any_version_range, is_valid_semver_range};

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
    assert!(is_valid_semver_range("*"));
    assert!(is_valid_semver_range("^1.0.0"));
    assert!(is_valid_semver_range("1.x"));
    assert!(is_valid_semver_range(">=1.0.0 <2.0.0"));
    assert!(is_valid_semver_range("^1.0.0 || ^2.0.0"));
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
