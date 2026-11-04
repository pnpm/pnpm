use pep440_rs::{Version, VersionSpecifiers};

use super::updated_existing_specifiers;

fn update(existing: &str, version: &str, force_bump: bool) -> Option<String> {
    let existing: VersionSpecifiers = existing.parse().unwrap();
    let version: Version = version.parse().unwrap();
    updated_existing_specifiers(&existing, &version, force_bump)
        .unwrap()
        .map(|specifiers| specifiers.to_string())
}

#[test]
fn compatible_release_keeps_its_precision() {
    assert_eq!(update("~=1.0", "1.5.9", false).as_deref(), Some("~=1.5"));
    assert_eq!(update("~=1.0", "2", true).as_deref(), Some("~=2.0"));
    assert_eq!(update("~=1.0.0", "1.5", false).as_deref(), Some("~=1.5.0"));
    assert_eq!(update("~=1.0", "2.0.0rc1", true).as_deref(), Some("~=2.0rc1"));
    assert_eq!(update("~=1.0", "2.0rc1", true).as_deref(), Some("~=2.0rc1"));
}

#[test]
fn compatible_release_keeps_a_selected_prerelease() {
    assert_eq!(update("~=1.0", "2.1.3rc1", true).as_deref(), Some("~=2.1rc1"));
}

#[test]
fn compatible_release_never_lowers_its_bound() {
    assert_eq!(update("~=1.2.post1", "1.2.0.1", false), None);
}

#[test]
fn compatible_release_follows_an_explicit_downgrade() {
    assert_eq!(update("~=3.0", "1.5", true).as_deref(), Some("~=1.5"));
}

#[test]
fn wildcard_equality_keeps_its_operator() {
    assert_eq!(update("==1.*", "2.3.1", true).as_deref(), Some("==2.*"));
    assert_eq!(update("==1.2.*", "2.3.1", true).as_deref(), Some("==2.3.*"));
    assert_eq!(update("==1.*", "2.1rc1", true).as_deref(), Some("==2.*, >=2.1rc1"));
    assert_eq!(update("==1.*", "1.4", true), None);
    assert_eq!(update("==1.*", "2.0", false), None);
}

#[test]
fn upper_bound_becomes_a_lower_bound_past_it() {
    assert_eq!(update("<2", "2.0", true).as_deref(), Some(">=2.0"));
}

#[test]
fn a_selected_prerelease_stays_admitted() {
    assert_eq!(update("==1.*", "1.4rc1", true).as_deref(), Some("==1.*, >=1.4rc1"));
    assert_eq!(update("<2", "1.4rc1", true).as_deref(), Some(">=1.4rc1, <2"));
    assert_eq!(update("==1.*, >=1.4rc1", "1.4rc1", true), None);
    assert_eq!(update(">=1.0", "1.4rc1", true).as_deref(), Some(">=1.4rc1"));
    assert_eq!(update(">=1.5rc1, <3.0rc1", "2.0rc1", false).as_deref(), Some(">=2.0rc1, <3.0rc1"));
}

#[test]
fn a_bounded_range_moves_its_lower_bound() {
    assert_eq!(update(">=1.0rc1, <2.0", "1.5", false).as_deref(), Some(">=1.5, <2.0"));
    assert_eq!(update("==2.*, >=2.1rc1", "2.3", false).as_deref(), Some("==2.*, >=2.3"));
    assert_eq!(update("==2.*, >=2.1rc1", "2.0", true).as_deref(), Some(">=2.0"));
    assert_eq!(update(">=1.0, <2", "2.0", true).as_deref(), Some(">=2.0"));
}
