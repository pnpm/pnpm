use node_semver::Version;
use pnpm_registry::{PackageVersion, RangeSpecStyle};

use super::{calc_prefixed_specifier, calc_specifier, calc_version_range};
use crate::infer_range_spec_style;

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
            calc_specifier(
                bare_specifier,
                None,
                Some("foo"),
                &picked("4.2.0"),
                RangeSpecStyle::Major,
            ),
            expected,
            "specifier for {bare_specifier}",
        );
    }
}

#[test]
fn falls_back_to_the_default_pin_when_none_is_declared() {
    for (default_pin, expected) in [
        (RangeSpecStyle::Major, "^4.2.0"),
        (RangeSpecStyle::Minor, "~4.2.0"),
        (RangeSpecStyle::Patch, "4.2.0"),
    ] {
        assert_eq!(
            calc_specifier("latest", None, Some("foo"), &picked("4.2.0"), default_pin),
            expected,
            "specifier for default pin {default_pin:?}",
        );
    }
}

#[test]
fn rewraps_an_npm_alias_around_the_new_range() {
    assert_eq!(
        calc_specifier(
            "npm:bar@^1.0.0",
            None,
            Some("foo"),
            &picked("4.2.0"),
            RangeSpecStyle::Major,
        ),
        "npm:bar@^4.2.0",
    );
    assert_eq!(
        calc_specifier(
            "npm:@types/table@6.0.0",
            None,
            Some("@types/zkochan__table"),
            &picked("7.0.0"),
            RangeSpecStyle::Major,
        ),
        "npm:@types/table@7.0.0",
    );
}

#[test]
fn an_alias_that_names_the_install_name_round_trips_as_a_bare_range() {
    for bare_specifier in ["npm:^1.0.0", "npm:foo@^1.0.0"] {
        assert_eq!(
            calc_specifier(
                bare_specifier,
                None,
                Some("foo"),
                &picked("4.2.0"),
                RangeSpecStyle::Major,
            ),
            "^4.2.0",
            "specifier for {bare_specifier}",
        );
    }
}

#[test]
fn a_prerelease_pick_keeps_the_declared_range_operator() {
    assert_eq!(
        calc_specifier(
            "5.0.0-rc.1",
            Some("^1.0.0"),
            Some("foo"),
            &picked("5.0.0-rc.1"),
            RangeSpecStyle::Major,
        ),
        "^5.0.0-rc.1",
    );
    // With no previous pin the prerelease stays exact rather than widened
    // to the default pin.
    assert_eq!(
        calc_specifier("latest", None, Some("foo"), &picked("5.0.0-rc.1"), RangeSpecStyle::Major),
        "5.0.0-rc.1",
    );
}

#[test]
fn calc_version_range_preserves_an_existing_prerelease_range_style() {
    let version = Version::parse("3.0.0-rc.11").expect("parse prerelease version");
    for (prev, expected) in [
        ("^3.0.0-rc.8", "^3.0.0-rc.11"),
        ("~3.0.0-rc.8", "~3.0.0-rc.11"),
        ("3.0.0-rc.8", "3.0.0-rc.11"),
        ("=3.0.0-rc.8", "=3.0.0-rc.11"),
        (">=3.0.0-rc.8", "3.0.0-rc.11"),
        ("2 || 3", "3.0.0-rc.11"),
    ] {
        assert_eq!(
            calc_version_range(&version, infer_range_spec_style(prev), None, RangeSpecStyle::Major),
            expected,
            "range for previous specifier {prev}",
        );
    }
    assert_eq!(calc_version_range(&version, None, None, RangeSpecStyle::Major), "3.0.0-rc.11");
}

#[test]
fn calc_version_range_ignores_the_requested_specifier_style_for_a_prerelease() {
    let prerelease = Version::parse("3.0.0-rc.11").expect("parse prerelease version");
    let spec_style = infer_range_spec_style("~3.0.0-rc.8");
    assert_eq!(
        calc_version_range(&prerelease, None, spec_style, RangeSpecStyle::Major),
        "3.0.0-rc.11",
    );
    let release = Version::parse("3.1.0").expect("parse release version");
    assert_eq!(calc_version_range(&release, None, spec_style, RangeSpecStyle::Major), "~3.1.0");
}

#[test]
fn a_prefixed_specifier_keeps_its_protocol_and_the_declared_range_operator() {
    for (bare_specifier, expected) in
        [("jsr:^1.0.0", "jsr:^4.2.0"), ("jsr:~1.0.0", "jsr:~4.2.0"), ("jsr:1.0.0", "jsr:4.2.0")]
    {
        assert_eq!(
            calc_prefixed_specifier(
                "jsr:",
                "@pnpm-e2e/foo",
                bare_specifier,
                None,
                Some("@pnpm-e2e/foo"),
                &picked("4.2.0"),
                RangeSpecStyle::Major,
            ),
            expected,
            "specifier for {bare_specifier}",
        );
    }
}

#[test]
fn an_aliased_prefixed_specifier_keeps_naming_the_package_it_resolves_through() {
    assert_eq!(
        calc_prefixed_specifier(
            "jsr:",
            "@pnpm-e2e/foo",
            "jsr:@pnpm-e2e/foo@1.0.0",
            None,
            Some("foo-from-jsr"),
            &picked("4.2.0"),
            RangeSpecStyle::Major,
        ),
        "jsr:@pnpm-e2e/foo@4.2.0",
    );
}

#[test]
fn an_unaliased_prefixed_specifier_carries_the_range_alone() {
    assert_eq!(
        calc_prefixed_specifier(
            "jsr:",
            "@pnpm-e2e/foo",
            "jsr:latest",
            None,
            None,
            &picked("4.2.0"),
            RangeSpecStyle::Minor,
        ),
        "jsr:~4.2.0",
    );
}

/// A dependency the manifest already declares under `^` or `~` keeps that
/// operator when it is requested at an exact version, as pnpm 11 does
/// (pnpm/pnpm#14745).
#[test]
fn the_previous_range_operator_wins_over_an_exact_request() {
    assert_eq!(
        calc_specifier(
            "19.3.0",
            Some("^19.2.8"),
            Some("react"),
            &picked("19.3.0"),
            RangeSpecStyle::Major,
        ),
        "^19.3.0",
    );
    assert_eq!(
        calc_specifier(
            "19.3.0",
            Some("~19.2.8"),
            Some("react"),
            &picked("19.3.0"),
            RangeSpecStyle::Major,
        ),
        "~19.3.0",
    );
    // No previous pin: the exact requested version stays exact.
    assert_eq!(
        calc_specifier("19.3.0", None, Some("react"), &picked("19.3.0"), RangeSpecStyle::Major),
        "19.3.0",
    );
}
