use super::{bumped_range, split_npm_alias};
use pnpm_lockfile::ImporterDepVersion;
use pnpm_registry::RangeSpecStyle;

fn bump(declared: &str, version: &str) -> Option<String> {
    let version = version.parse::<ImporterDepVersion>().expect("parse the resolved version");
    bumped_range(declared, &version, RangeSpecStyle::Major)
}

#[test]
fn keeps_the_declared_range_operator() {
    assert_eq!(bump("^1.0.0", "1.2.0").as_deref(), Some("^1.2.0"));
    assert_eq!(bump("~1.0.0", "1.0.3").as_deref(), Some("~1.0.3"));
    assert_eq!(bump("=1.0.0", "1.2.0").as_deref(), Some("=1.2.0"));
    assert_eq!(bump("1.0.0", "1.2.0").as_deref(), Some("1.2.0"));
}

#[test]
fn falls_back_to_the_default_operator_when_the_declaration_pins_none() {
    assert_eq!(bump(">=1.0.0", "3.1.0").as_deref(), Some("^3.1.0"));
    assert_eq!(bump("1 || 2", "1.2.0").as_deref(), Some("^1.2.0"));
    assert_eq!(bump("*", "2.1.0").as_deref(), Some("^2.1.0"));
}

#[test]
fn a_declaration_that_already_names_the_version_is_left_alone() {
    assert_eq!(bump("^1.2.0", "1.2.0"), None);
    assert_eq!(bump("1.2.0", "1.2.0"), None);
}

#[test]
fn a_dist_tag_keeps_tracking_its_tag() {
    assert_eq!(bump("latest", "3.0.1"), None);
    assert_eq!(bump("next", "3.0.1"), None);
}

#[test]
fn a_prerelease_pick_is_pinned_exactly() {
    assert_eq!(bump("^1.0.0", "2.0.0-beta.1").as_deref(), Some("2.0.0-beta.1"));
}

#[test]
fn an_npm_alias_keeps_pointing_at_the_same_package() {
    assert_eq!(
        bump("npm:is-positive@^3.0.0", "is-positive@3.1.0").as_deref(),
        Some("npm:is-positive@^3.1.0"),
    );
    assert_eq!(
        bump("npm:@scope/pkg@~1.0.0", "@scope/pkg@1.0.4").as_deref(),
        Some("npm:@scope/pkg@~1.0.4"),
    );
}

#[test]
fn declarations_of_other_protocols_are_left_alone() {
    for declared in [
        "workspace:*",
        "workspace:^1.0.0",
        "link:../foo",
        "file:../foo.tgz",
        "catalog:default",
        "jsr:^1.0.0",
        "https://example.com/foo.tgz",
    ] {
        assert_eq!(bump(declared, "1.2.0"), None, "{declared} should be left alone");
    }
}

#[test]
fn a_version_with_no_semver_to_pin_is_left_alone() {
    assert_eq!(bump("^1.0.0", "link:../foo"), None);
    assert_eq!(bump("^1.0.0", "file:../foo(react@18.0.0)"), None);
}

#[test]
fn a_peer_suffix_is_dropped_from_the_written_range() {
    assert_eq!(bump("^17.0.0", "17.0.2(react@17.0.2)").as_deref(), Some("^17.0.2"));
}

#[test]
fn npm_aliases_split_into_the_prefix_they_keep() {
    assert_eq!(split_npm_alias("^1.0.0"), Some(("", "^1.0.0")));
    assert_eq!(split_npm_alias("npm:foo@^1.0.0"), Some(("npm:foo@", "^1.0.0")));
    assert_eq!(split_npm_alias("npm:@scope/foo@^1.0.0"), Some(("npm:@scope/foo@", "^1.0.0")));
    assert_eq!(split_npm_alias("npm:^1.0.0"), Some(("npm:", "^1.0.0")));
    assert_eq!(split_npm_alias("workspace:^1.0.0"), None);
}
