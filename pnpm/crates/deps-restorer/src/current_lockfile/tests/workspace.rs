use super::{
    empty_lockfile, importer_link, importer_map, include_all, key, package_metadata, pkg,
    snapshot_with_deps, ver,
};
use crate::SkippedSnapshots;
use pnpm_lockfile::{
    ImporterDepVersion, Lockfile, ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec,
    SnapshotDepRef, SnapshotEntry,
};
use pnpm_modules_yaml::IncludedDependencies;
use pretty_assertions::assert_eq;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

#[test]
fn skipped_snapshot_pruned_from_snapshots_and_importer_optional() {
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
    skipped.add_optional_excluded(key("drop", "1.0.0"));

    let filtered = super::super::filter_lockfile_for_current(&lockfile, include_all(), &skipped);

    let snaps = filtered.snapshots.as_ref().unwrap();
    assert!(snaps.contains_key(&key("keep", "1.0.0")));
    assert!(!snaps.contains_key(&key("drop", "1.0.0")), "skipped snapshot must be pruned");

    let imp = filtered.importers.get(".").unwrap();
    assert!(
        imp.optional_dependencies.as_ref().unwrap().is_empty(),
        "importer optional_dependencies entry pointing at a pruned snapshot must be removed",
    );
    assert!(imp.dependencies.as_ref().unwrap().contains_key(&pkg("keep")));
}
#[test]
fn include_optional_false_clears_importer_section() {
    let mut importers = HashMap::new();
    importers.insert(
        ".".to_string(),
        ProjectSnapshot {
            dependencies: Some(importer_map(&[("keep", "1.0.0")])),
            optional_dependencies: Some(importer_map(&[("opt", "1.0.0")])),
            ..Default::default()
        },
    );

    let mut snapshots = HashMap::new();
    snapshots.insert(key("keep", "1.0.0"), SnapshotEntry::default());
    snapshots.insert(key("opt", "1.0.0"), SnapshotEntry::default());

    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    // Match the install pipeline: when `--no-optional` is passed,
    // `InstallFrozenLockfile::run` also adds optional-only snapshots
    // to `skipped`, so the BFS doesn't reach `opt@1.0.0` via the
    // importer root either.
    let mut skipped = SkippedSnapshots::new();
    skipped.add_optional_excluded(key("opt", "1.0.0"));
    let include = IncludedDependencies {
        dependencies: true,
        dev_dependencies: true,
        optional_dependencies: false,
    };

    let filtered = super::super::filter_lockfile_for_current(&lockfile, include, &skipped);

    assert!(filtered.importers.get(".").unwrap().optional_dependencies.is_none());
    assert!(!filtered.snapshots.as_ref().unwrap().contains_key(&key("opt", "1.0.0")));
    assert!(filtered.snapshots.as_ref().unwrap().contains_key(&key("keep", "1.0.0")));
}
#[test]
fn user_excluded_packages_filtered_to_surviving_metadata_keys() {
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

    let mut packages = HashMap::new();
    packages.insert(key("keep", "1.0.0"), package_metadata("keep"));
    packages.insert(key("drop", "1.0.0"), package_metadata("drop"));

    let lockfile = Lockfile {
        importers,
        snapshots: Some(snapshots),
        packages: Some(packages),
        ..empty_lockfile()
    };

    let mut skipped = SkippedSnapshots::new();
    skipped.add_optional_excluded(key("drop", "1.0.0"));

    let filtered = super::super::filter_lockfile_for_current(&lockfile, include_all(), &skipped);

    let pkgs = filtered.packages.as_ref().unwrap();
    assert!(pkgs.contains_key(&key("keep", "1.0.0")));
    assert!(
        !pkgs.contains_key(&key("drop", "1.0.0")),
        "metadata for a user-excluded snapshot must also be pruned",
    );
}
#[test]
fn link_optional_entries_survive_post_filter() {
    let mut opt_map = ResolvedDependencyMap::new();
    opt_map.insert(
        pkg("workspace-pkg"),
        ResolvedDependencySpec {
            specifier: "workspace:*".to_string(),
            version: ImporterDepVersion::Link("../workspace-pkg".to_string()),
        },
    );

    let mut importers = HashMap::new();
    importers.insert(
        ".".to_string(),
        ProjectSnapshot { optional_dependencies: Some(opt_map), ..Default::default() },
    );

    let lockfile = Lockfile { importers, snapshots: Some(HashMap::new()), ..empty_lockfile() };

    let filtered = super::super::filter_lockfile_for_current(
        &lockfile,
        include_all(),
        &SkippedSnapshots::new(),
    );

    let opt = filtered.importers.get(".").unwrap().optional_dependencies.as_ref().unwrap();
    assert!(
        opt.contains_key(&pkg("workspace-pkg")),
        "link: importer entries must survive the optional-deps post-filter",
    );
}
#[test]
fn materialization_closure_keeps_importer_links_shallow_and_traverses_snapshot_links() {
    let nested_id = "packages/nested/a".to_string();
    let linked_id = "packages/b".to_string();
    let shared_id = "packages/shared".to_string();
    let disjoint_id = "packages/disjoint".to_string();

    let mut nested_deps = importer_map(&[("start", "1.0.0")]);
    nested_deps.insert(pkg("linked-workspace"), importer_link("../../../packages/b"));
    let mut linked_deps = importer_map(&[("linked-pkg", "1.0.0")]);
    linked_deps.insert(pkg("back-to-start"), importer_link("../nested/a"));

    let importers = HashMap::from([
        (
            nested_id.clone(),
            ProjectSnapshot {
                dependencies: Some(nested_deps),
                dev_dependencies: Some(importer_map(&[("dev-only", "1.0.0")])),
                optional_dependencies: Some(importer_map(&[("optional-only", "1.0.0")])),
                ..Default::default()
            },
        ),
        (
            linked_id.clone(),
            ProjectSnapshot { dependencies: Some(linked_deps), ..Default::default() },
        ),
        (
            shared_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("shared-pkg", "1.0.0")])),
                ..Default::default()
            },
        ),
        (
            disjoint_id.clone(),
            ProjectSnapshot {
                dependencies: Some(importer_map(&[("disjoint", "1.0.0")])),
                ..Default::default()
            },
        ),
    ]);

    let snapshots = HashMap::from([
        (
            key("start", "1.0.0"),
            SnapshotEntry {
                dependencies: Some(HashMap::from([
                    (pkg("child"), SnapshotDepRef::Plain(ver("1.0.0"))),
                    (pkg("common"), SnapshotDepRef::Plain(ver("1.0.0"))),
                    (pkg("shared-workspace"), SnapshotDepRef::Link("packages/shared".to_string())),
                ])),
                ..Default::default()
            },
        ),
        (key("child", "1.0.0"), snapshot_with_deps(&[("start", "1.0.0")])),
        (key("linked-pkg", "1.0.0"), snapshot_with_deps(&[("common", "1.0.0")])),
        (key("common", "1.0.0"), SnapshotEntry::default()),
        (
            key("shared-pkg", "1.0.0"),
            SnapshotEntry {
                dependencies: Some(HashMap::from([(
                    pkg("back-to-nested"),
                    SnapshotDepRef::Link("packages/nested/a".to_string()),
                )])),
                ..Default::default()
            },
        ),
        (key("dev-only", "1.0.0"), SnapshotEntry::default()),
        (key("optional-only", "1.0.0"), SnapshotEntry::default()),
        (key("disjoint", "1.0.0"), SnapshotEntry::default()),
    ]);
    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };
    let selected = HashSet::from([nested_id.clone()]);
    let included = IncludedDependencies {
        dependencies: true,
        dev_dependencies: false,
        optional_dependencies: false,
    };

    let closure = super::super::materialization_closure(
        &lockfile,
        Path::new("/workspace"),
        &selected,
        included,
        &SkippedSnapshots::new(),
    );

    assert_eq!(closure.importer_ids, HashSet::from([nested_id.clone(), shared_id]));
    assert!(!closure.importer_ids.contains(&linked_id));
    assert!(!closure.importer_ids.contains(&disjoint_id));
    let nested = closure.lockfile.importers.get(&nested_id).unwrap();
    assert!(nested.dev_dependencies.is_none());
    assert!(nested.optional_dependencies.is_none());
    let reached = closure.lockfile.snapshots.as_ref().unwrap();
    for reached_key in [
        key("start", "1.0.0"),
        key("child", "1.0.0"),
        key("common", "1.0.0"),
        key("shared-pkg", "1.0.0"),
    ] {
        assert!(reached.contains_key(&reached_key), "missing {reached_key}");
    }
    assert!(!reached.contains_key(&key("linked-pkg", "1.0.0")));
    assert!(!reached.contains_key(&key("dev-only", "1.0.0")));
    assert!(!reached.contains_key(&key("optional-only", "1.0.0")));
    assert!(!reached.contains_key(&key("disjoint", "1.0.0")));
}
#[test]
fn materialization_closure_does_not_follow_reverse_workspace_links() {
    let selected_id = "packages/shared".to_string();
    let dependent_id = "packages/app".to_string();
    let mut dependent_dependencies = importer_map(&[("app-only", "1.0.0")]);
    dependent_dependencies.insert(pkg("shared"), importer_link("../shared"));
    let lockfile = Lockfile {
        importers: HashMap::from([
            (
                selected_id.clone(),
                ProjectSnapshot {
                    dependencies: Some(importer_map(&[("shared-only", "1.0.0")])),
                    ..Default::default()
                },
            ),
            (
                dependent_id.clone(),
                ProjectSnapshot {
                    dependencies: Some(dependent_dependencies),
                    ..Default::default()
                },
            ),
        ]),
        snapshots: Some(HashMap::from([
            (key("shared-only", "1.0.0"), SnapshotEntry::default()),
            (key("app-only", "1.0.0"), SnapshotEntry::default()),
        ])),
        ..empty_lockfile()
    };

    let closure = super::super::materialization_closure(
        &lockfile,
        Path::new("/workspace"),
        &HashSet::from([selected_id.clone()]),
        include_all(),
        &SkippedSnapshots::new(),
    );

    assert_eq!(closure.importer_ids, HashSet::from([selected_id]));
    assert!(!closure.importer_ids.contains(&dependent_id));
    let snapshots = closure.lockfile.snapshots.as_ref().unwrap();
    assert!(snapshots.contains_key(&key("shared-only", "1.0.0")));
    assert!(!snapshots.contains_key(&key("app-only", "1.0.0")));
}
