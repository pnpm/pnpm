use super::{empty_lockfile, importer_link, importer_map, include_all, key, pkg};
use crate::SkippedSnapshots;
use pnpm_lockfile::{Lockfile, ProjectSnapshot, SnapshotDepRef, SnapshotEntry};
use pnpm_modules_yaml::IncludedDependencies;
use pretty_assertions::assert_eq;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

#[test]
fn materialization_closure_excludes_optional_snapshot_link_when_optionals_are_disabled() {
    let selected_id = "packages/selected".to_string();
    let linked_id = "packages/linked".to_string();
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
                linked_id.clone(),
                ProjectSnapshot {
                    dependencies: Some(importer_map(&[("linked-pkg", "1.0.0")])),
                    ..Default::default()
                },
            ),
        ]),
        snapshots: Some(HashMap::from([
            (
                key("parent", "1.0.0"),
                SnapshotEntry {
                    optional_dependencies: Some(HashMap::from([(
                        pkg("linked"),
                        SnapshotDepRef::Link("packages/linked".to_string()),
                    )])),
                    ..Default::default()
                },
            ),
            (key("linked-pkg", "1.0.0"), SnapshotEntry::default()),
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
    assert!(!closure.importer_ids.contains(&linked_id));
    let snapshots = closure.lockfile.snapshots.as_ref().unwrap();
    assert!(snapshots.contains_key(&key("parent", "1.0.0")));
    assert!(!snapshots.contains_key(&key("linked-pkg", "1.0.0")));
}
#[test]
fn materialization_closure_ignores_unknown_link_targets() {
    let mut dependencies = importer_map(&[("parent", "1.0.0")]);
    dependencies.insert(pkg("outside"), importer_link("../outside"));
    let importers = HashMap::from([
        (
            Lockfile::ROOT_IMPORTER_KEY.to_string(),
            ProjectSnapshot { dependencies: Some(dependencies), ..Default::default() },
        ),
        ("packages/known".to_string(), ProjectSnapshot::default()),
    ]);
    let snapshots = HashMap::from([(
        key("parent", "1.0.0"),
        SnapshotEntry {
            dependencies: Some(HashMap::from([(
                pkg("missing"),
                SnapshotDepRef::Link("packages/missing".to_string()),
            )])),
            ..Default::default()
        },
    )]);
    let lockfile = Lockfile { importers, snapshots: Some(snapshots), ..empty_lockfile() };

    let closure = super::super::materialization_closure(
        &lockfile,
        Path::new("/workspace"),
        &HashSet::from([Lockfile::ROOT_IMPORTER_KEY.to_string()]),
        include_all(),
        &SkippedSnapshots::new(),
    );

    assert_eq!(closure.importer_ids, HashSet::from([Lockfile::ROOT_IMPORTER_KEY.to_string()]));
    assert_eq!(closure.lockfile.importers.len(), 1);
}
