use std::{collections::HashMap, str::FromStr};

use pacquet_lockfile::{
    ImporterDepVersion, PkgName, PkgVerPeer, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
};

use super::{
    ImporterDiffKey, LockfileDiff, SnapshotDiff, diff_importer, diff_snapshot_entry,
    render_dry_run_report,
};

fn pkg(name: &str) -> PkgName {
    PkgName::from_str(name).expect("parse PkgName")
}

fn ver(version: &str) -> PkgVerPeer {
    version.parse().expect("parse PkgVerPeer")
}

/// Build an importer dependency map from `(alias, specifier, version)` triples.
fn importer_map(entries: &[(&str, &str, &str)]) -> ResolvedDependencyMap {
    entries
        .iter()
        .map(|(alias, specifier, version)| {
            (
                pkg(alias),
                ResolvedDependencySpec {
                    specifier: (*specifier).to_string(),
                    version: ImporterDepVersion::Regular(ver(version)),
                },
            )
        })
        .collect()
}

#[test]
fn empty_diff_reports_no_changes() {
    let report = render_dry_run_report(&LockfileDiff::default());
    assert!(report.contains("up to date"), "got: {report}");
    assert!(report.contains("no changes"), "got: {report}");
}

#[test]
fn non_empty_diff_lists_importer_and_package_changes() {
    let diff = LockfileDiff {
        importers: vec![SnapshotDiff {
            id: ".".to_string(),
            added: vec![("is-negative".to_string(), "1.0.0".to_string())],
            removed: vec![],
            updated: vec![("is-positive".to_string(), "1.0.0".to_string(), "2.0.0".to_string())],
        }],
        added_packages: vec!["is-negative@1.0.0".to_string()],
        removed_packages: vec![],
        updated_packages: vec![],
    };
    let report = render_dry_run_report(&diff);
    assert!(report.contains("+ is-negative 1.0.0"), "got: {report}");
    assert!(report.contains("is-positive 1.0.0 -> 2.0.0"), "got: {report}");
    assert!(report.contains("+ is-negative@1.0.0"), "got: {report}");
}

#[test]
fn snapshot_wiring_change_is_detected() {
    let old = SnapshotEntry::default();
    let mut new = SnapshotEntry::default();
    let unchanged = diff_snapshot_entry("is-positive@1.0.0".to_string(), &old, &new);
    assert!(unchanged.is_empty(), "identical snapshots must not differ: {unchanged:?}");

    new.dependencies =
        Some(HashMap::from([(pkg("is-positive"), SnapshotDepRef::Plain(ver("1.0.0")))]));
    let changed = diff_snapshot_entry("is-positive@1.0.0".to_string(), &old, &new);
    assert_eq!(changed.added, vec![("is-positive".to_string(), "1.0.0".to_string())]);
}

#[test]
fn group_move_is_reported_even_when_version_is_unchanged() {
    let old = ProjectSnapshot {
        dev_dependencies: Some(importer_map(&[("is-positive", "^1.0.0", "1.0.0")])),
        ..Default::default()
    };
    let new = ProjectSnapshot {
        dependencies: Some(importer_map(&[("is-positive", "^1.0.0", "1.0.0")])),
        ..Default::default()
    };
    let diff = diff_importer(".", Some(&old), Some(&new), ImporterDiffKey::Specifier);
    assert!(!diff.is_empty(), "a dev -> prod move must register as a change: {diff:?}");
}

#[test]
fn specifier_only_change_is_reported() {
    let old = ProjectSnapshot {
        dependencies: Some(importer_map(&[("is-positive", "^1.0.0", "1.0.0")])),
        ..Default::default()
    };
    let new = ProjectSnapshot {
        dependencies: Some(importer_map(&[("is-positive", "~1.0.0", "1.0.0")])),
        ..Default::default()
    };
    let diff = diff_importer(".", Some(&old), Some(&new), ImporterDiffKey::Specifier);
    assert!(!diff.is_empty(), "a specifier-only change must be reported: {diff:?}");

    let by_version = diff_importer(".", Some(&old), Some(&new), ImporterDiffKey::Version);
    assert!(
        by_version.is_empty(),
        "dedupe --check compares resolved versions, so a specifier-only change is not one: {by_version:?}",
    );
}

/// `dedupe` rewrites peer-resolved versions without touching the manifest
/// specifiers, so only [`ImporterDiffKey::Version`] sees the change.
#[test]
fn peer_suffix_change_is_reported_by_version() {
    let old = ProjectSnapshot {
        dependencies: Some(importer_map(&[("is-positive", "^1.0.0", "1.0.0")])),
        ..Default::default()
    };
    let new = ProjectSnapshot {
        dependencies: Some(importer_map(&[("is-positive", "^1.0.0", "1.0.0(is-negative@1.0.0)")])),
        ..Default::default()
    };
    let diff = diff_importer(".", Some(&old), Some(&new), ImporterDiffKey::Version);
    assert_eq!(
        diff.updated,
        vec![(
            "is-positive".to_string(),
            "1.0.0".to_string(),
            "1.0.0(is-negative@1.0.0)".to_string(),
        )],
    );

    let by_specifier = diff_importer(".", Some(&old), Some(&new), ImporterDiffKey::Specifier);
    assert!(by_specifier.is_empty(), "the specifier is unchanged: {by_specifier:?}");
}
