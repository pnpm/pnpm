use super::{
    empty_lockfile, importer_map, include_all, key, package_metadata, pkg, snapshot_with_deps, ver,
};
use crate::SkippedSnapshots;
use pnpm_lockfile::{Lockfile, ProjectSnapshot, SnapshotDepRef, SnapshotEntry};
use pnpm_modules_yaml::IncludedDependencies;
use pretty_assertions::assert_eq;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

#[test]
fn transitive_under_skipped_snapshot_is_pruned() {
    let mut importers = HashMap::new();
    importers.insert(
        ".".to_string(),
        ProjectSnapshot {
            optional_dependencies: Some(importer_map(&[("parent", "1.0.0")])),
            ..Default::default()
        },
    );

    let mut snapshots = HashMap::new();
    snapshots.insert(key("parent", "1.0.0"), snapshot_with_deps(&[("child", "1.0.0")]));
    snapshots.insert(key("child", "1.0.0"), SnapshotEntry::default());

    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let mut skipped = SkippedSnapshots::new();
    skipped.add_optional_excluded(key("parent", "1.0.0"));

    let filtered = super::super::filter_lockfile_for_current(&lockfile, include_all(), &skipped);

    let snaps = filtered.snapshots.as_ref().unwrap();
    assert!(!snaps.contains_key(&key("parent", "1.0.0")));
    assert!(
        !snaps.contains_key(&key("child", "1.0.0")),
        "transitive under a skipped parent must be pruned too",
    );
}
#[test]
fn snapshot_reachable_via_kept_path_survives() {
    let mut importers = HashMap::new();
    importers.insert(
        ".".to_string(),
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("kept-parent", "1.0.0")])),
            optional_dependencies: Some(importer_map(&[("opt-parent", "1.0.0")])),
            ..Default::default()
        },
    );

    let mut snapshots = HashMap::new();
    snapshots.insert(key("kept-parent", "1.0.0"), snapshot_with_deps(&[("shared", "1.0.0")]));
    snapshots.insert(key("opt-parent", "1.0.0"), snapshot_with_deps(&[("shared", "1.0.0")]));
    snapshots.insert(key("shared", "1.0.0"), SnapshotEntry::default());

    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let mut skipped = SkippedSnapshots::new();
    skipped.add_optional_excluded(key("opt-parent", "1.0.0"));

    let filtered = super::super::filter_lockfile_for_current(&lockfile, include_all(), &skipped);

    let snaps = filtered.snapshots.as_ref().unwrap();
    assert!(snaps.contains_key(&key("kept-parent", "1.0.0")));
    assert!(
        snaps.contains_key(&key("shared", "1.0.0")),
        "shared snapshot must survive because the kept parent still references it",
    );
    assert!(!snaps.contains_key(&key("opt-parent", "1.0.0")));
}
/// Retaining them is what keeps the current lockfile comparable to the
/// wanted one — see [`SkippedSnapshots::transient_only`].
#[test]
fn installability_skipped_entries_are_preserved() {
    let mut importers = HashMap::new();
    importers.insert(
        ".".to_string(),
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("keep", "1.0.0")])),
            optional_dependencies: Some(importer_map(&[("drop", "1.0.0")])),
            ..Default::default()
        },
    );

    let mut snapshots = HashMap::new();
    snapshots.insert(key("keep", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(key("drop", "1.0.0"), snapshot_with_deps(&[("child", "1.0.0")]));
    snapshots.insert(key("child", "1.0.0"), SnapshotEntry::default());

    let mut packages = HashMap::new();
    packages.insert(key("keep", "1.0.0"), package_metadata("keep"));
    packages.insert(key("drop", "1.0.0"), package_metadata("drop"));
    packages.insert(key("child", "1.0.0"), package_metadata("child"));

    let lockfile = Lockfile {
        importers,
        snapshots: Some(snapshots),
        packages: Some(packages),
        ..empty_lockfile()
    };

    // The recorded skip set is the reachability closure of the direct
    // skip, matching what `.modules.yaml.skipped` holds after an
    // install that dropped `drop@1.0.0`.
    let mut skipped = SkippedSnapshots::new();
    skipped.insert_installability(key("drop", "1.0.0"));
    skipped.insert_installability(key("child", "1.0.0"));

    let filtered = super::super::filter_lockfile_for_current(&lockfile, include_all(), &skipped);

    assert_eq!(filtered, lockfile);
}
/// A fetch failure is recorded nowhere else, so dropping the snapshot
/// here is what makes the next install retry it.
#[test]
fn fetch_failed_snapshot_is_pruned() {
    let mut importers = HashMap::new();
    importers.insert(
        ".".to_string(),
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("keep", "1.0.0")])),
            optional_dependencies: Some(importer_map(&[("drop", "1.0.0")])),
            ..Default::default()
        },
    );

    let mut snapshots = HashMap::new();
    snapshots.insert(key("keep", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(key("drop", "1.0.0"), SnapshotEntry::default());

    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let mut skipped = SkippedSnapshots::new();
    skipped.add_fetch_failed(key("drop", "1.0.0"));

    let filtered = super::super::filter_lockfile_for_current(&lockfile, include_all(), &skipped);

    let snaps = filtered.snapshots.as_ref().unwrap();
    assert!(snaps.contains_key(&key("keep", "1.0.0")));
    assert!(!snaps.contains_key(&key("drop", "1.0.0")));
    let imp = filtered.importers.get(".").unwrap();
    assert!(imp.optional_dependencies.as_ref().unwrap().is_empty());
}
#[test]
fn empty_skipped_and_full_include_is_identity_for_reachables() {
    let mut importers = HashMap::new();
    importers.insert(
        ".".to_string(),
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("a", "1.0.0")])),
            ..Default::default()
        },
    );

    let mut snapshots = HashMap::new();
    snapshots.insert(key("a", "1.0.0"), snapshot_with_deps(&[("b", "1.0.0")]));
    snapshots.insert(key("b", "1.0.0"), SnapshotEntry::default());

    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let filtered = super::super::filter_lockfile_for_current(
        &lockfile,
        include_all(),
        &SkippedSnapshots::new(),
    );

    let snaps = filtered.snapshots.as_ref().unwrap();
    assert_eq!(snaps.len(), 2);
    assert!(snaps.contains_key(&key("a", "1.0.0")));
    assert!(snaps.contains_key(&key("b", "1.0.0")));

    let imp = filtered.importers.get(".").unwrap();
    assert!(imp.dependencies.as_ref().unwrap().contains_key(&pkg("a")));
}
#[test]
fn orphan_snapshots_are_pruned() {
    let importers = HashMap::from([(
        ".".to_string(),
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("a", "1.0.0")])),
            ..Default::default()
        },
    )]);

    let mut snapshots = HashMap::new();
    snapshots.insert(key("a", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(key("orphan", "1.0.0"), SnapshotEntry::default());

    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let filtered = super::super::filter_lockfile_for_current(
        &lockfile,
        include_all(),
        &SkippedSnapshots::new(),
    );

    let snaps = filtered.snapshots.as_ref().unwrap();
    assert_eq!(snaps.len(), 1);
    assert!(snaps.contains_key(&key("a", "1.0.0")));
    assert!(!snaps.contains_key(&key("orphan", "1.0.0")));
}
#[test]
fn materialization_closure_excludes_transitive_optional_shared_snapshot_when_disabled() {
    let selected_id = "packages/selected".to_string();
    let unselected_id = "packages/unselected".to_string();
    let lockfile = Lockfile {
        importers: HashMap::from([
            (
                selected_id.clone(),
                ProjectSnapshot {
                    dependencies: Some(importer_map(&[("parent", "1.0.0")])),
                    ..Default::default()
                },
            ),
            (
                unselected_id,
                ProjectSnapshot {
                    dependencies: Some(importer_map(&[("shared", "1.0.0")])),
                    ..Default::default()
                },
            ),
        ]),
        snapshots: Some(HashMap::from([
            (
                key("parent", "1.0.0"),
                SnapshotEntry {
                    optional_dependencies: Some(HashMap::from([(
                        pkg("shared"),
                        SnapshotDepRef::Plain(ver("1.0.0")),
                    )])),
                    ..Default::default()
                },
            ),
            (key("shared", "1.0.0"), SnapshotEntry::default()),
        ])),
        ..empty_lockfile()
    };
    let included = IncludedDependencies {
        dependencies: true,
        dev_dependencies: true,
        optional_dependencies: false,
    };

    let closure = super::super::materialization_closure(
        &lockfile,
        Path::new("/workspace"),
        &HashSet::from([selected_id.clone()]),
        included,
        &SkippedSnapshots::new(),
    );

    assert_eq!(closure.importer_ids, HashSet::from([selected_id]));
    let snapshots = closure.lockfile.snapshots.as_ref().unwrap();
    assert!(snapshots.contains_key(&key("parent", "1.0.0")));
    assert!(!snapshots.contains_key(&key("shared", "1.0.0")));
}
#[test]
fn skip_closure_extends_installability_roots() {
    let importer_id = ".".to_string();
    let lockfile = Lockfile {
        importers: HashMap::from([(
            importer_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("parent", "1.0.0")])),
                ..Default::default()
            },
        )]),
        snapshots: Some(HashMap::from([
            (key("parent", "1.0.0"), snapshot_with_deps(&[("child", "1.0.0")])),
            (key("child", "1.0.0"), SnapshotEntry::default()),
        ])),
        ..empty_lockfile()
    };

    let mut skipped = SkippedSnapshots::from_strings(["parent@1.0.0"]);
    super::super::extend_skipped_with_dependency_closure(
        &mut skipped,
        &lockfile,
        Path::new("/workspace"),
        &HashSet::from([importer_id]),
        include_all(),
    );

    assert!(
        skipped.iter_installability().any(|key| key.to_string() == "child@1.0.0"),
        "a snapshot only reachable through an installability skip joins the persisted set",
    );
}
/// Transient exclusions (`--no-optional`, `--no-runtime`, failed fetches)
/// must not seed the persisted closure: a later install's `.modules.yaml`
/// seed would otherwise keep their subtrees skipped after the transient
/// condition is gone.
#[test]
fn skip_closure_ignores_transient_roots() {
    let importer_id = ".".to_string();
    let lockfile = Lockfile {
        importers: HashMap::from([(
            importer_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("parent", "1.0.0")])),
                ..Default::default()
            },
        )]),
        snapshots: Some(HashMap::from([
            (key("parent", "1.0.0"), snapshot_with_deps(&[("child", "1.0.0")])),
            (key("child", "1.0.0"), SnapshotEntry::default()),
        ])),
        ..empty_lockfile()
    };

    let mut skipped = SkippedSnapshots::new();
    skipped.add_optional_excluded(key("parent", "1.0.0"));
    super::super::extend_skipped_with_dependency_closure(
        &mut skipped,
        &lockfile,
        Path::new("/workspace"),
        &HashSet::from([importer_id]),
        include_all(),
    );

    assert_eq!(
        skipped.iter_installability().count(),
        0,
        "a transient exclusion must contribute nothing to the persisted skip set",
    );
    assert!(!skipped.contains(&key("child", "1.0.0")));
}
