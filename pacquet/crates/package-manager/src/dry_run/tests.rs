use std::{collections::HashMap, str::FromStr};

use pacquet_lockfile::{
    ImporterDepVersion, PkgName, PkgVerPeer, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
};

use super::{
    ImporterDiff, LockfileDiff, diff_importer, render_dry_run_report, snapshot_wiring_differs,
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
        importers: vec![ImporterDiff {
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
    assert!(!snapshot_wiring_differs(&old, &new), "identical snapshots must not differ");

    new.dependencies =
        Some(HashMap::from([(pkg("is-positive"), SnapshotDepRef::Plain(ver("1.0.0")))]));
    assert!(snapshot_wiring_differs(&old, &new), "a new dependency edge must register as a change");
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
    let diff = diff_importer(".", Some(&old), Some(&new));
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
    let diff = diff_importer(".", Some(&old), Some(&new));
    assert!(!diff.is_empty(), "a specifier-only change must be reported: {diff:?}");
}
