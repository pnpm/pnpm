use super::{UpdateTargets, VersionLine};

fn targets(entries: &[(&str, Option<&str>)]) -> UpdateTargets {
    entries
        .iter()
        .map(|(name, version)| ((*name).to_string(), version.and_then(VersionLine::parse)))
        .collect()
}

fn version(version: &str) -> node_semver::Version {
    version.parse().expect("parse version")
}

#[test]
fn merge_adds_the_lines_of_scoped_targets() {
    let mut merged = targets(&[("foo", Some("1.2.3"))]);
    merged.merge(targets(&[("foo", Some("2.0.0")), ("bar", Some("0.1.0"))]));
    assert!(merged.covers("foo", Some(&version("1.9.0"))));
    assert!(merged.covers("foo", Some(&version("2.1.0"))));
    assert!(!merged.covers("foo", Some(&version("3.0.0"))));
    assert!(merged.covers("bar", Some(&version("0.1.5"))));
    assert!(!merged.covers("bar", Some(&version("0.2.0"))));
}

#[test]
fn merge_widens_a_scoped_target_to_every_version() {
    let mut merged = targets(&[("foo", Some("1.2.3"))]);
    merged.merge(targets(&[("foo", None)]));
    assert!(merged.covers("foo", Some(&version("3.0.0"))));
}

#[test]
fn merge_never_narrows_a_target_that_covers_every_version() {
    let mut merged = targets(&[("foo", None)]);
    merged.merge(targets(&[("foo", Some("1.2.3"))]));
    assert!(merged.covers("foo", Some(&version("3.0.0"))));
}
