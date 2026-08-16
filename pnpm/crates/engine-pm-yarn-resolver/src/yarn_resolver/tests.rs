use super::{YarnResolverError, pick_release};
use crate::read_yarn_releases::parse_releases;

fn releases() -> Vec<crate::read_yarn_releases::YarnRelease> {
    parse_releases(
        r#"[
          { "tag_name": "v6.0.0-rc.19", "assets": [] },
          { "tag_name": "v6.0.0-rc.18", "assets": [] },
          { "tag_name": "v7.0.0", "assets": [] },
          { "tag_name": "not-a-version", "assets": [] }
        ]"#,
    )
    .expect("parse the release list")
}

fn picked(version_spec: &str) -> Option<String> {
    pick_release(&releases(), version_spec).map(|release| release.version.clone())
}

#[test]
fn the_newest_release_wins_for_an_open_specifier() {
    for version_spec in ["latest", "*", "", "  "] {
        assert_eq!(picked(version_spec).as_deref(), Some("7.0.0"), "{version_spec}");
    }
}

#[test]
fn a_range_picks_the_newest_release_inside_it() {
    assert_eq!(picked("^7.0.0").as_deref(), Some("7.0.0"));
    assert_eq!(picked("7.0.0").as_deref(), Some("7.0.0"));
}

/// Yarn 6 exists only as release candidates, so a plain `6` range would
/// match nothing under semver's prerelease rule.
#[test]
fn a_range_falls_back_to_prereleases_when_nothing_stable_matches() {
    assert_eq!(picked("^6.0.0").as_deref(), Some("6.0.0-rc.19"));
    assert_eq!(picked("6").as_deref(), Some("6.0.0-rc.19"));
}

#[test]
fn an_exact_prerelease_is_matched_directly() {
    assert_eq!(picked("6.0.0-rc.18").as_deref(), Some("6.0.0-rc.18"));
}

#[test]
fn a_range_no_release_satisfies_resolves_to_nothing() {
    assert_eq!(picked("^9.0.0"), None);
    assert_eq!(picked("not a range"), None);
}

/// A specifier comes from a manifest, so it can carry credentials — the
/// message a user sees must not.
#[test]
fn a_credential_bearing_specifier_is_redacted_in_the_error() {
    let error = YarnResolverError::ResolutionFailure {
        spec: pnpm_network::redact_and_sanitize("runtime:https://user:hunter2@example.test/yarn"),
    };

    let rendered = error.to_string();
    assert!(!rendered.contains("hunter2"), "{rendered}");
}
