use node_semver::Version;

use super::{Bump, ReleaseType, inc, parse_bump};

fn version(text: &str) -> Version {
    text.parse().expect("valid version")
}

/// The node-semver `inc()` table the bumps must reproduce, including the
/// finalize-a-prerelease shortcuts and the preid handling.
#[test]
fn inc_matches_node_semver() {
    let cases: &[(&str, ReleaseType, Option<&str>, &str)] = &[
        ("1.2.3", ReleaseType::Major, None, "2.0.0"),
        ("2.0.0-alpha.1", ReleaseType::Major, None, "2.0.0"),
        ("2.1.0-alpha.1", ReleaseType::Major, None, "3.0.0"),
        ("1.2.3", ReleaseType::Minor, None, "1.3.0"),
        ("1.3.0-beta", ReleaseType::Minor, None, "1.3.0"),
        ("1.3.1-beta", ReleaseType::Minor, None, "1.4.0"),
        ("1.2.3", ReleaseType::Patch, None, "1.2.4"),
        ("1.2.4-rc.1", ReleaseType::Patch, None, "1.2.4"),
        ("1.2.3", ReleaseType::Premajor, Some("alpha"), "2.0.0-alpha.0"),
        ("1.2.3", ReleaseType::Premajor, None, "2.0.0-0"),
        ("1.2.3", ReleaseType::Preminor, Some("alpha"), "1.3.0-alpha.0"),
        ("1.2.3", ReleaseType::Prepatch, Some("alpha"), "1.2.4-alpha.0"),
        ("1.0.0", ReleaseType::Prerelease, Some("alpha"), "1.0.1-alpha.0"),
        ("1.0.1-alpha.0", ReleaseType::Prerelease, Some("alpha"), "1.0.1-alpha.1"),
        ("1.0.1-alpha.1", ReleaseType::Prerelease, Some("beta"), "1.0.1-beta.0"),
        ("1.0.0-beta", ReleaseType::Prerelease, Some("beta"), "1.0.0-beta.0"),
        ("1.0.0-beta.fooblz", ReleaseType::Prerelease, Some("beta"), "1.0.0-beta.0"),
        ("1.0.0", ReleaseType::Prerelease, None, "1.0.1-0"),
        ("1.0.0-1", ReleaseType::Prerelease, None, "1.0.0-2"),
        ("1.0.0+build.5", ReleaseType::Patch, None, "1.0.1"),
    ];
    for (current, release, preid, expected) in cases {
        let bumped = inc(&version(current), *release, *preid).to_string();
        assert_eq!(&bumped, expected, "inc({current}, {release:?}, {preid:?})");
    }
}

#[test]
fn parse_bump_accepts_versions_and_release_types() {
    let Ok(Bump::Explicit(explicit)) = parse_bump("1.2.3") else {
        panic!("1.2.3 should parse as an explicit version");
    };
    assert_eq!(explicit.to_string(), "1.2.3");

    let Ok(Bump::Explicit(prerelease)) = parse_bump("2.0.0-beta.1") else {
        panic!("2.0.0-beta.1 should parse as an explicit version");
    };
    assert_eq!(prerelease.to_string(), "2.0.0-beta.1");

    assert!(matches!(parse_bump("major"), Ok(Bump::Release(ReleaseType::Major))));
    assert!(matches!(parse_bump("prerelease"), Ok(Bump::Release(ReleaseType::Prerelease))));
    assert!(parse_bump("not-a-version").is_err(), "junk should be rejected");
}

/// `semver.valid` accepts a leading `v` and returns the cleaned version, so
/// the argument form `pnpm version v1.2.3` sets `1.2.3`.
#[test]
fn parse_bump_strips_a_leading_v() {
    let Ok(Bump::Explicit(explicit)) = parse_bump("v1.2.3") else {
        panic!("v1.2.3 should parse as an explicit version");
    };
    assert_eq!(explicit.to_string(), "1.2.3");
}
