use super::{optional_children_match, optional_children_match_with};
use crate::{CreateVirtualStoreError, SkippedSnapshots, VirtualStoreLayout};
use pnpm_lockfile::{PackageKey, PkgName, SnapshotDepRef, SnapshotEntry};
use std::{collections::HashMap, fs, io};

#[test]
fn optional_child_probe_propagates_io_errors() {
    let snapshot_key: PackageKey = "parent@1.0.0".parse().expect("parse snapshot key");
    let snapshot = SnapshotEntry {
        optional_dependencies: Some(HashMap::from([(
            PkgName::parse("optional-child").expect("parse alias"),
            SnapshotDepRef::Plain("1.0.0".parse().expect("parse version")),
        )])),
        ..SnapshotEntry::default()
    };
    let layout = VirtualStoreLayout::legacy("virtual-store", 120);
    let error = optional_children_match_with(
        &snapshot_key,
        &snapshot,
        &layout,
        &SkippedSnapshots::default(),
        true,
        true,
        |_, _| Err(io::Error::new(io::ErrorKind::PermissionDenied, "fixture denial")),
    )
    .expect_err("permission errors must abort the warm-slot probe");

    assert!(matches!(
        error,
        CreateVirtualStoreError::InspectOptionalDependency { error, .. }
            if error.kind() == io::ErrorKind::PermissionDenied
    ));
}

#[test]
fn invalid_optional_child_entries_do_not_match() {
    let temp_dir = tempfile::tempdir().expect("create temp directory");
    let snapshot_key: PackageKey = "parent@1.0.0".parse().expect("parse snapshot key");
    let snapshot = SnapshotEntry {
        optional_dependencies: Some(HashMap::from([(
            PkgName::parse("optional-child").expect("parse alias"),
            SnapshotDepRef::Plain("1.0.0".parse().expect("parse version")),
        )])),
        ..SnapshotEntry::default()
    };
    let layout = VirtualStoreLayout::legacy(temp_dir.path().join("virtual-store"), 120);
    let child_path = layout.slot_dir(&snapshot_key).join("node_modules").join("optional-child");
    fs::create_dir_all(child_path.parent().expect("child path has a parent"))
        .expect("create slot modules directory");
    let target = temp_dir.path().join("optional-target");
    fs::create_dir(&target).expect("create optional target");
    pnpm_fs::symlink_dir(&target, &child_path).expect("link optional child");

    assert!(
        optional_children_match(
            &snapshot_key,
            &snapshot,
            &layout,
            &SkippedSnapshots::default(),
            true,
            true,
        )
        .expect("inspect valid optional child"),
    );

    fs::remove_dir(&target).expect("remove optional target");
    assert!(
        !optional_children_match(
            &snapshot_key,
            &snapshot,
            &layout,
            &SkippedSnapshots::default(),
            true,
            true,
        )
        .expect("inspect dangling optional child"),
    );
    assert!(
        !optional_children_match(
            &snapshot_key,
            &snapshot,
            &layout,
            &SkippedSnapshots::default(),
            true,
            false,
        )
        .expect("inspect unexpected dangling optional child"),
    );

    pnpm_fs::remove_symlink_dir(&child_path).expect("remove dangling optional child");
    fs::write(&child_path, "not a directory").expect("create invalid optional child");
    assert!(
        !optional_children_match(
            &snapshot_key,
            &snapshot,
            &layout,
            &SkippedSnapshots::default(),
            true,
            true,
        )
        .expect("inspect non-directory optional child"),
    );
}
