use super::{
    empty_lockfile, importer_link, importer_map, include_all, key, lockfile_with_top_level,
    package_metadata, pkg, snapshot_with_deps,
};
use crate::SkippedSnapshots;
use pnpm_lockfile::{
    Lockfile, ProjectSnapshot, ResolvedDependencyMap, SnapshotDepRef, SnapshotEntry,
};
use pretty_assertions::assert_eq;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

#[test]
fn nested_importer_links_stay_shallow_and_snapshot_links_use_lockfile_base() {
    let nested_id = "packages/nested/a".to_string();
    let importer_target_id = "packages/importer-target".to_string();
    let snapshot_target_id = "packages/snapshot-target".to_string();
    let provider_version = "1.0.0(peer@2.0.0)";
    let mut dependencies = importer_map(&[("provider", provider_version)]);
    dependencies.insert(pkg("importer-target"), importer_link("../../../packages/importer-target"));
    let importers = HashMap::from([
        (
            nested_id.clone(),
            ProjectSnapshot { dependencies: Some(dependencies), ..Default::default() },
        ),
        (importer_target_id.clone(), ProjectSnapshot::default()),
        (snapshot_target_id.clone(), ProjectSnapshot::default()),
    ]);
    let snapshots = HashMap::from([(
        key("provider", provider_version),
        SnapshotEntry {
            dependencies: Some(HashMap::from([(
                pkg("snapshot-target"),
                SnapshotDepRef::Link("packages/snapshot-target".to_string()),
            )])),
            ..Default::default()
        },
    )]);
    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let closure = super::super::materialization_closure(
        &lockfile,
        Path::new("/workspace"),
        &HashSet::from([nested_id.clone()]),
        include_all(),
        &SkippedSnapshots::new(),
    );

    assert_eq!(closure.importer_ids, HashSet::from([nested_id, snapshot_target_id]));
    assert!(!closure.importer_ids.contains(&importer_target_id));
}
#[test]
fn merge_filtered_wanted_lockfile_refreshes_all_importers_when_global_inputs_change() {
    let selected_id = "packages/selected".to_string();
    let retained_id = "packages/retained".to_string();
    let removed_id = "packages/removed".to_string();
    let new_id = "packages/new".to_string();

    let prior_retained = ProjectSnapshot {
        specifiers: Some(HashMap::from([("retained".to_string(), "prior".to_string())])),
        dependencies: Some(importer_map(&[("retained", "1.0.0"), ("shared", "1.0.0")])),
        ..Default::default()
    };
    let mut previous = lockfile_with_top_level("prior", 0);
    previous.importers = HashMap::from([
        (
            selected_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("selected-old", "1.0.0")])),
                ..Default::default()
            },
        ),
        (retained_id.clone(), prior_retained),
        (
            removed_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("removed", "1.0.0")])),
                ..Default::default()
            },
        ),
    ]);
    previous.snapshots = Some(HashMap::from([
        (key("selected-old", "1.0.0"), SnapshotEntry::default()),
        (key("retained", "1.0.0"), snapshot_with_deps(&[("retained-child", "1.0.0")])),
        (key("retained-child", "1.0.0"), SnapshotEntry::default()),
        (key("shared", "1.0.0"), snapshot_with_deps(&[("old-child", "1.0.0")])),
        (key("old-child", "1.0.0"), SnapshotEntry::default()),
        (key("removed", "1.0.0"), SnapshotEntry::default()),
    ]));
    previous.packages = Some(
        previous
            .snapshots
            .as_ref()
            .unwrap()
            .keys()
            .map(|package_key| {
                (package_key.without_peer(), package_metadata(&format!("prior-{package_key}")))
            })
            .collect(),
    );

    let fresh_selected = ProjectSnapshot {
        specifiers: Some(HashMap::from([("selected-new".to_string(), "fresh".to_string())])),
        dependencies: Some(importer_map(&[("selected-new", "2.0.0"), ("shared", "1.0.0")])),
        ..Default::default()
    };
    let fresh_new = ProjectSnapshot {
        dependencies: Some(importer_map(&[("new-pkg", "1.0.0")])),
        ..Default::default()
    };
    let mut fresh = lockfile_with_top_level("fresh", 1);
    let fresh_retained = ProjectSnapshot {
        dependencies: Some(importer_map(&[("fresh-only", "2.0.0")])),
        ..Default::default()
    };
    fresh.importers = HashMap::from([
        (selected_id.clone(), fresh_selected.clone()),
        (retained_id.clone(), fresh_retained.clone()),
        (new_id.clone(), fresh_new.clone()),
    ]);
    let fresh_shared = snapshot_with_deps(&[("fresh-child", "2.0.0")]);
    fresh.snapshots = Some(HashMap::from([
        (key("selected-new", "2.0.0"), SnapshotEntry::default()),
        (key("shared", "1.0.0"), fresh_shared.clone()),
        (key("fresh-child", "2.0.0"), SnapshotEntry::default()),
        (key("fresh-only", "2.0.0"), SnapshotEntry::default()),
        (key("new-pkg", "1.0.0"), SnapshotEntry::default()),
    ]));
    fresh.packages = Some(
        fresh
            .snapshots
            .as_ref()
            .unwrap()
            .keys()
            .map(|package_key| {
                (package_key.without_peer(), package_metadata(&format!("fresh-{package_key}")))
            })
            .collect(),
    );
    let expected_lockfile_version = fresh.lockfile_version;
    let expected_settings = fresh.settings.clone();
    let expected_catalogs = fresh.catalogs.clone();
    let expected_overrides = fresh.overrides.clone();
    let expected_package_extensions_checksum = fresh.package_extensions_checksum.clone();
    let expected_pnpmfile_checksum = fresh.pnpmfile_checksum.clone();
    let expected_ignored_optional_dependencies = fresh.ignored_optional_dependencies.clone();
    let expected_patched_dependencies = fresh.patched_dependencies.clone();
    let expected_shared_metadata =
        fresh.packages.as_ref().unwrap().get(&key("shared", "1.0.0")).cloned();

    let merged = super::super::merge_filtered_wanted_lockfile(
        Some(&previous),
        fresh,
        &HashSet::from([selected_id.clone(), retained_id.clone(), new_id.clone()]),
        &HashSet::from([selected_id.clone()]),
        Path::new("/workspace"),
    )
    .expect("the fresh lockfile contains every required importer");

    assert_eq!(merged.importers.get(&selected_id), Some(&fresh_selected));
    assert_eq!(merged.importers.get(&retained_id), Some(&fresh_retained));
    assert_eq!(merged.importers.get(&new_id), Some(&fresh_new));
    assert!(!merged.importers.contains_key(&removed_id));
    let snapshots = merged.snapshots.as_ref().unwrap();
    assert!(!snapshots.contains_key(&key("retained", "1.0.0")));
    assert!(!snapshots.contains_key(&key("retained-child", "1.0.0")));
    assert!(snapshots.contains_key(&key("fresh-only", "2.0.0")));
    assert!(!snapshots.contains_key(&key("selected-old", "1.0.0")));
    assert_eq!(snapshots.get(&key("shared", "1.0.0")), Some(&fresh_shared));
    assert!(snapshots.contains_key(&key("fresh-child", "2.0.0")));
    assert!(!snapshots.contains_key(&key("old-child", "1.0.0")));
    assert_eq!(
        merged.packages.as_ref().unwrap().get(&key("shared", "1.0.0")),
        expected_shared_metadata.as_ref(),
    );
    assert_eq!(merged.lockfile_version, expected_lockfile_version);
    assert_eq!(merged.settings, expected_settings);
    assert_eq!(merged.catalogs, expected_catalogs);
    assert_eq!(merged.overrides, expected_overrides);
    assert_eq!(merged.package_extensions_checksum, expected_package_extensions_checksum);
    assert_eq!(merged.pnpmfile_checksum, expected_pnpmfile_checksum);
    assert_eq!(merged.ignored_optional_dependencies, expected_ignored_optional_dependencies);
    assert_eq!(merged.patched_dependencies, expected_patched_dependencies);
}
#[test]
fn merge_filtered_wanted_lockfile_preserves_unselected_importers_when_global_inputs_match() {
    let selected_id = "packages/selected".to_string();
    let retained_id = "packages/retained".to_string();
    let prior_retained = ProjectSnapshot {
        dependencies: Some(importer_map(&[("retained-old", "1.0.0")])),
        ..Default::default()
    };
    let mut previous = lockfile_with_top_level("same", 0);
    previous.importers = HashMap::from([
        (
            selected_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("selected-old", "1.0.0")])),
                ..Default::default()
            },
        ),
        (retained_id.clone(), prior_retained.clone()),
    ]);
    previous.snapshots = Some(HashMap::from([
        (key("selected-old", "1.0.0"), SnapshotEntry::default()),
        (key("retained-old", "1.0.0"), SnapshotEntry::default()),
    ]));

    let fresh_selected = ProjectSnapshot {
        dependencies: Some(importer_map(&[("selected-new", "2.0.0")])),
        ..Default::default()
    };
    let mut fresh = lockfile_with_top_level("same", 0);
    fresh.importers = HashMap::from([
        (selected_id.clone(), fresh_selected.clone()),
        (
            retained_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("retained-fresh", "2.0.0")])),
                ..Default::default()
            },
        ),
    ]);
    fresh.snapshots = Some(HashMap::from([
        (key("selected-new", "2.0.0"), SnapshotEntry::default()),
        (key("retained-fresh", "2.0.0"), SnapshotEntry::default()),
    ]));

    let merged = super::super::merge_filtered_wanted_lockfile(
        Some(&previous),
        fresh,
        &HashSet::from([selected_id.clone(), retained_id.clone()]),
        &HashSet::from([selected_id.clone()]),
        Path::new("/workspace"),
    )
    .expect("fresh lockfile contains every real importer");

    assert_eq!(merged.importers.get(&selected_id), Some(&fresh_selected));
    assert_eq!(merged.importers.get(&retained_id), Some(&prior_retained));
    let snapshots = merged.snapshots.as_ref().expect("merged snapshots");
    assert!(snapshots.contains_key(&key("selected-new", "2.0.0")));
    assert!(snapshots.contains_key(&key("retained-old", "1.0.0")));
    assert!(!snapshots.contains_key(&key("selected-old", "1.0.0")));
    assert!(!snapshots.contains_key(&key("retained-fresh", "2.0.0")));
}
#[test]
fn merge_filtered_wanted_lockfile_rejects_a_missing_selected_importer() {
    let error = super::super::merge_filtered_wanted_lockfile(
        None,
        empty_lockfile(),
        &HashSet::from(["packages/selected".to_string()]),
        &HashSet::from(["packages/selected".to_string()]),
        Path::new("/workspace"),
    )
    .expect_err("a selected importer missing from the fresh lockfile must be rejected");

    assert_eq!(error.to_string(), "fresh lockfile is missing importer packages/selected");
}
#[test]
fn merge_filtered_wanted_lockfile_keeps_a_dependency_free_importer() {
    // A dependency-free importer is *present-but-empty* in the fresh
    // lockfile, not absent: pnpm's `pruneLockfile` records it as
    // `{ specifiers: {} }` and the pnpr resolver returns every requested
    // importer. So an unfiltered full-workspace merge that lists a dep-free
    // project in `real_importer_ids` finds its empty snapshot and merges it
    // without a `MissingImporter` error (that error is reserved for an
    // importer genuinely absent from the fresh lockfile — a partial
    // resolution — as the sibling test above covers).
    let app_id = "packages/app".to_string();
    let lib_id = "packages/lib".to_string();
    let fresh_app = ProjectSnapshot {
        dependencies: Some(importer_map(&[("dep", "1.0.0")])),
        ..Default::default()
    };
    let mut fresh = lockfile_with_top_level("same", 0);
    fresh.importers = HashMap::from([
        (app_id.clone(), fresh_app.clone()),
        (lib_id.clone(), ProjectSnapshot::default()),
    ]);
    fresh.snapshots = Some(HashMap::from([(key("dep", "1.0.0"), SnapshotEntry::default())]));

    let all = HashSet::from([app_id.clone(), lib_id.clone()]);
    let merged = super::super::merge_filtered_wanted_lockfile(
        None,
        fresh,
        &all,
        &all,
        Path::new("/workspace"),
    )
    .expect("a dependency-free importer present as an empty snapshot must not error");

    assert_eq!(merged.importers.get(&app_id), Some(&fresh_app));
    assert_eq!(merged.importers.get(&lib_id), Some(&ProjectSnapshot::default()));
}
#[test]
fn merge_filtered_current_lockfile_preserves_prior_importers_across_sequential_runs() {
    let first_id = "packages/first".to_string();
    let second_id = "packages/second".to_string();
    let mut previous = lockfile_with_top_level("prior", 0);
    let prior_first = ProjectSnapshot {
        dependencies: Some(importer_map(&[("first-old", "1.0.0")])),
        ..Default::default()
    };
    previous.importers = HashMap::from([
        (first_id.clone(), prior_first.clone()),
        (
            second_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("second-old", "1.0.0")])),
                ..Default::default()
            },
        ),
    ]);
    previous.snapshots = Some(HashMap::from([
        (key("first-old", "1.0.0"), SnapshotEntry::default()),
        (key("second-old", "1.0.0"), SnapshotEntry::default()),
    ]));

    let mut wanted = lockfile_with_top_level("fresh", 1);
    let fresh_second = ProjectSnapshot {
        dependencies: Some(importer_map(&[("second-new", "2.0.0")])),
        ..Default::default()
    };
    wanted.importers = HashMap::from([
        (
            first_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("fresh-only-first", "2.0.0")])),
                ..Default::default()
            },
        ),
        (second_id.clone(), fresh_second.clone()),
    ]);
    wanted.snapshots = Some(HashMap::from([
        (key("fresh-only-first", "2.0.0"), SnapshotEntry::default()),
        (key("second-new", "2.0.0"), SnapshotEntry::default()),
    ]));
    let expected_settings = wanted.settings.clone();

    let merged = super::super::merge_filtered_current_lockfile(
        Some(&previous),
        &wanted,
        &HashSet::from([second_id.clone()]),
        include_all(),
        &SkippedSnapshots::new(),
        Path::new("/workspace"),
    );

    assert_eq!(merged.importers.get(&first_id), Some(&prior_first));
    assert_eq!(merged.importers.get(&second_id), Some(&fresh_second));
    let snapshots = merged.snapshots.as_ref().unwrap();
    assert!(snapshots.contains_key(&key("first-old", "1.0.0")));
    assert!(snapshots.contains_key(&key("second-new", "2.0.0")));
    assert!(!snapshots.contains_key(&key("second-old", "1.0.0")));
    assert!(!snapshots.contains_key(&key("fresh-only-first", "2.0.0")));
    assert_eq!(merged.settings, expected_settings);
}
#[test]
fn merge_filtered_current_lockfile_does_not_restore_a_skipped_selected_snapshot() {
    let selected_id = "packages/selected".to_string();
    let selected_importer = ProjectSnapshot {
        dependencies: Some(importer_map(&[("parent", "1.0.0")])),
        ..Default::default()
    };
    let snapshots = HashMap::from([
        (key("parent", "1.0.0"), snapshot_with_deps(&[("child", "1.0.0")])),
        (key("child", "1.0.0"), SnapshotEntry::default()),
    ]);
    let packages = HashMap::from([
        (key("parent", "1.0.0"), package_metadata("parent")),
        (key("child", "1.0.0"), package_metadata("child")),
    ]);
    let previous = Lockfile {
        importers: HashMap::from([(selected_id.clone(), selected_importer.clone())]),
        snapshots: Some(snapshots.clone()),
        packages: Some(packages.clone()),
        ..empty_lockfile()
    };
    let wanted = Lockfile {
        importers: HashMap::from([(selected_id.clone(), selected_importer)]),
        snapshots: Some(snapshots),
        packages: Some(packages),
        ..empty_lockfile()
    };
    let mut skipped = SkippedSnapshots::new();
    skipped.add_fetch_failed(key("parent", "1.0.0"));

    let merged = super::super::merge_filtered_current_lockfile(
        Some(&previous),
        &wanted,
        &HashSet::from([selected_id]),
        include_all(),
        &skipped,
        Path::new("/workspace"),
    );

    let snapshots = merged.snapshots.as_ref().unwrap();
    assert!(!snapshots.contains_key(&key("parent", "1.0.0")));
    assert!(!snapshots.contains_key(&key("child", "1.0.0")));
    let packages = merged.packages.as_ref().unwrap();
    assert!(packages.contains_key(&key("parent", "1.0.0")));
    assert!(packages.contains_key(&key("child", "1.0.0")));
}
/// The filtered-install path has to agree with
/// [`super::super::filter_lockfile_for_current`] here — it writes the same file.
#[test]
fn merge_filtered_current_lockfile_keeps_an_installability_skipped_snapshot() {
    let selected_id = "packages/selected".to_string();
    let selected_importer = ProjectSnapshot {
        optional_dependencies: Some(importer_map(&[("parent", "1.0.0")])),
        ..Default::default()
    };
    let snapshots = HashMap::from([
        (key("parent", "1.0.0"), snapshot_with_deps(&[("child", "1.0.0")])),
        (key("child", "1.0.0"), SnapshotEntry::default()),
    ]);
    let packages = HashMap::from([
        (key("parent", "1.0.0"), package_metadata("parent")),
        (key("child", "1.0.0"), package_metadata("child")),
    ]);
    let wanted = Lockfile {
        importers: HashMap::from([(selected_id.clone(), selected_importer)]),
        snapshots: Some(snapshots),
        packages: Some(packages),
        ..empty_lockfile()
    };
    let mut skipped = SkippedSnapshots::new();
    skipped.insert_installability(key("parent", "1.0.0"));
    skipped.insert_installability(key("child", "1.0.0"));

    let merged = super::super::merge_filtered_current_lockfile(
        None,
        &wanted,
        &HashSet::from([selected_id]),
        include_all(),
        &skipped,
        Path::new("/workspace"),
    );

    assert_eq!(merged, wanted);
}
#[test]
fn merge_filtered_current_lockfile_uses_one_fresh_shared_snapshot() {
    let retained_id = "packages/retained".to_string();
    let selected_id = "packages/selected".to_string();
    let mut previous = empty_lockfile();
    previous.importers = HashMap::from([(
        retained_id.clone(),
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("shared", "1.0.0")])),
            ..Default::default()
        },
    )]);
    previous.snapshots = Some(HashMap::from([
        (key("shared", "1.0.0"), snapshot_with_deps(&[("old-child", "1.0.0")])),
        (key("old-child", "1.0.0"), SnapshotEntry::default()),
    ]));
    previous.packages = Some(HashMap::from([
        (key("shared", "1.0.0"), package_metadata("old-shared")),
        (key("old-child", "1.0.0"), package_metadata("old-child")),
    ]));

    let mut wanted = empty_lockfile();
    wanted.importers = HashMap::from([(
        selected_id.clone(),
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("shared", "1.0.0")])),
            ..Default::default()
        },
    )]);
    let fresh_shared = snapshot_with_deps(&[("fresh-child", "2.0.0")]);
    let fresh_shared_metadata = package_metadata("fresh-shared");
    wanted.snapshots = Some(HashMap::from([
        (key("shared", "1.0.0"), fresh_shared.clone()),
        (key("fresh-child", "2.0.0"), SnapshotEntry::default()),
    ]));
    wanted.packages = Some(HashMap::from([
        (key("shared", "1.0.0"), fresh_shared_metadata.clone()),
        (key("fresh-child", "2.0.0"), package_metadata("fresh-child")),
    ]));

    let merged = super::super::merge_filtered_current_lockfile(
        Some(&previous),
        &wanted,
        &HashSet::from([selected_id.clone()]),
        include_all(),
        &SkippedSnapshots::new(),
        Path::new("/workspace"),
    );

    assert!(merged.importers.contains_key(&retained_id));
    assert!(merged.importers.contains_key(&selected_id));
    let snapshots = merged.snapshots.as_ref().unwrap();
    assert_eq!(snapshots.get(&key("shared", "1.0.0")), Some(&fresh_shared));
    assert!(snapshots.contains_key(&key("fresh-child", "2.0.0")));
    assert!(!snapshots.contains_key(&key("old-child", "1.0.0")));
    assert_eq!(
        merged.packages.as_ref().unwrap().get(&key("shared", "1.0.0")),
        Some(&fresh_shared_metadata),
    );
}
#[test]
fn merge_filtered_current_lockfile_preserves_shallow_link_target_importers() {
    let selected_id = "packages/a".to_string();
    let linked_id = "packages/b".to_string();
    let previous_linked = ProjectSnapshot {
        dependencies: Some(importer_map(&[("linked-old", "1.0.0")])),
        ..Default::default()
    };
    let mut previous = empty_lockfile();
    previous.importers = HashMap::from([(linked_id.clone(), previous_linked.clone())]);
    previous.snapshots =
        Some(HashMap::from([(key("linked-old", "1.0.0"), SnapshotEntry::default())]));

    let mut selected_dependencies = ResolvedDependencyMap::new();
    selected_dependencies.insert(pkg("linked"), importer_link("../b"));
    let mut wanted = empty_lockfile();
    wanted.importers = HashMap::from([
        (
            selected_id.clone(),
            ProjectSnapshot { dependencies: Some(selected_dependencies), ..Default::default() },
        ),
        (
            linked_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("linked-new", "2.0.0")])),
                ..Default::default()
            },
        ),
    ]);
    wanted.snapshots =
        Some(HashMap::from([(key("linked-new", "2.0.0"), SnapshotEntry::default())]));

    let merged = super::super::merge_filtered_current_lockfile(
        Some(&previous),
        &wanted,
        &HashSet::from([selected_id]),
        include_all(),
        &SkippedSnapshots::new(),
        Path::new("/workspace"),
    );

    assert_eq!(merged.importers.get(&linked_id), Some(&previous_linked));
    assert!(merged.snapshots.as_ref().unwrap().contains_key(&key("linked-old", "1.0.0")));
    assert!(!merged.snapshots.as_ref().unwrap().contains_key(&key("linked-new", "2.0.0")));
}
