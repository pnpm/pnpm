use std::{collections::HashMap, str::FromStr};

use pnpm_lockfile::{
    ComVer, ImporterDepVersion, Lockfile, LockfileVersion, PackageKey, PkgName, PkgVerPeer,
    ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
};

use super::{
    ImporterDiffKey, LockfileDiff, SnapshotDiff, diff_importer, diff_lockfiles,
    diff_snapshot_entry, render_dry_run_report,
};

fn pkg(name: &str) -> PkgName {
    PkgName::from_str(name).expect("parse PkgName")
}

fn ver(version: &str) -> PkgVerPeer {
    version.parse().expect("parse PkgVerPeer")
}

/// The whole-lockfile diff `dedupe --check` reports: an importer whose
/// resolved version moved, the snapshots that came and went with it, and
/// one that stayed but was rewired.
#[test]
fn lockfile_diff_covers_importers_and_snapshots() {
    let old = lockfile(
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("is-positive", "^1.0.0", "1.0.0")])),
            ..Default::default()
        },
        &[("is-positive@1.0.0", snapshot(&[])), ("shared@1.0.0", snapshot(&[("dep", "1.0.0")]))],
    );
    let new = lockfile(
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("is-positive", "^1.0.0", "2.0.0")])),
            ..Default::default()
        },
        &[("is-positive@2.0.0", snapshot(&[])), ("shared@1.0.0", snapshot(&[("dep", "2.0.0")]))],
    );

    let diff = diff_lockfiles(Some(&old), Some(&new), ImporterDiffKey::Version);

    assert_eq!(diff.importers.len(), 1);
    assert_eq!(diff.importers[0].id, ".");
    assert_eq!(
        diff.importers[0].updated,
        vec![("is-positive".to_string(), "1.0.0".to_string(), "2.0.0".to_string())],
    );
    assert_eq!(diff.added_packages, vec!["is-positive@2.0.0".to_string()]);
    assert_eq!(diff.removed_packages, vec!["is-positive@1.0.0".to_string()]);
    assert_eq!(diff.updated_packages.len(), 1);
    assert_eq!(diff.updated_packages[0].id, "shared@1.0.0");
    assert_eq!(
        diff.updated_packages[0].updated,
        vec![("dep".to_string(), "1.0.0".to_string(), "2.0.0".to_string())],
    );
}

/// An identical lockfile is not a change — the equality short-circuit and
/// the per-alias walk have to agree on that.
#[test]
fn identical_lockfiles_yield_an_empty_diff() {
    let importer = ProjectSnapshot {
        dependencies: Some(importer_map(&[("is-positive", "^1.0.0", "1.0.0")])),
        ..Default::default()
    };
    let one = lockfile(importer.clone(), &[("is-positive@1.0.0", snapshot(&[("dep", "1.0.0")]))]);
    let other = lockfile(importer, &[("is-positive@1.0.0", snapshot(&[("dep", "1.0.0")]))]);

    let diff = diff_lockfiles(Some(&one), Some(&other), ImporterDiffKey::Version);
    assert!(diff.is_empty(), "got: {diff:?}");
}

fn key(package_key: &str) -> PackageKey {
    package_key.parse().expect("parse PackageKey")
}

/// A lockfile with one root importer and the given `snapshots:` entries.
fn lockfile(root: ProjectSnapshot, snapshots: &[(&str, SnapshotEntry)]) -> Lockfile {
    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer { major: 9, minor: 0 })
            .expect("lockfile version 9.0"),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: HashMap::from([(".".to_string(), root)]),
        packages: None,
        snapshots: Some(snapshots.iter().map(|(id, entry)| (key(id), entry.clone())).collect()),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    }
}

/// A `snapshots:` entry with the given `dependencies` edges.
fn snapshot(deps: &[(&str, &str)]) -> SnapshotEntry {
    SnapshotEntry {
        dependencies: Some(
            deps.iter()
                .map(|(alias, version)| (pkg(alias), SnapshotDepRef::Plain(ver(version))))
                .collect(),
        ),
        ..Default::default()
    }
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
    // pnpm merges every group's diff into one alias-keyed map, so the move
    // is one verdict — the last group's — not an addition contradicted by a
    // removal of the same alias.
    assert_eq!(diff.added, vec![]);
    assert_eq!(diff.removed, vec![("is-positive".to_string(), "^1.0.0".to_string())]);
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
