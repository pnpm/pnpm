use pacquet_registry::{PackageVersion, PinnedVersion};

use super::calc_specifier;

fn picked(version: &str) -> PackageVersion {
    serde_json::from_value(serde_json::json!({
        "name": "foo",
        "version": version,
        "dist": { "tarball": "https://registry.npmjs.org/foo/-/foo.tgz" },
    }))
    .expect("build a package version")
}

#[test]
fn keeps_the_range_operator_the_dependency_already_declared() {
    for (bare_specifier, expected) in
        [("^1.0.0", "^4.2.0"), ("~1.0.0", "~4.2.0"), ("1.0.0", "4.2.0"), ("*", "^4.2.0")]
    {
        assert_eq!(
            calc_specifier(bare_specifier, Some("foo"), &picked("4.2.0"), PinnedVersion::Major),
            expected,
            "specifier for {bare_specifier}",
        );
    }
}

#[test]
fn falls_back_to_the_default_pin_when_none_is_declared() {
    for (default_pin, expected) in [
        (PinnedVersion::Major, "^4.2.0"),
        (PinnedVersion::Minor, "~4.2.0"),
        (PinnedVersion::Patch, "4.2.0"),
    ] {
        assert_eq!(
            calc_specifier("latest", Some("foo"), &picked("4.2.0"), default_pin),
            expected,
            "specifier for default pin {default_pin:?}",
        );
    }
}

#[test]
fn rewraps_an_npm_alias_around_the_new_range() {
    assert_eq!(
        calc_specifier("npm:bar@^1.0.0", Some("foo"), &picked("4.2.0"), PinnedVersion::Major),
        "npm:bar@^4.2.0",
    );
    assert_eq!(
        calc_specifier(
            "npm:@types/table@6.0.0",
            Some("@types/zkochan__table"),
            &picked("7.0.0"),
            PinnedVersion::Major,
        ),
        "npm:@types/table@7.0.0",
    );
}

#[test]
fn an_alias_that_names_the_install_name_round_trips_as_a_bare_range() {
    for bare_specifier in ["npm:^1.0.0", "npm:foo@^1.0.0"] {
        assert_eq!(
            calc_specifier(bare_specifier, Some("foo"), &picked("4.2.0"), PinnedVersion::Major),
            "^4.2.0",
            "specifier for {bare_specifier}",
        );
    }
}

/// A prerelease has no meaningful range operator, so it is pinned exactly
/// whatever the declared specifier or the default asks for.
#[test]
fn a_prerelease_pick_is_written_exactly() {
    assert_eq!(
        calc_specifier("^1.0.0", Some("foo"), &picked("5.0.0-rc.1"), PinnedVersion::Major),
        "5.0.0-rc.1",
    );
}
