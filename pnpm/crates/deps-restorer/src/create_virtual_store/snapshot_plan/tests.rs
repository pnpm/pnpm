use super::optional_children_match_with;
use crate::{CreateVirtualStoreError, SkippedSnapshots, VirtualStoreLayout};
use pacquet_lockfile::{PackageKey, PkgName, SnapshotDepRef, SnapshotEntry};
use std::{collections::HashMap, io};

#[test]
fn optional_child_probe_propagates_io_errors() {
    let snapshot_key: PackageKey = "parent@1.0.0".parse().expect("parse snapshot key");
    let snapshot = SnapshotEntry {
        optional_dependencies: Some(HashMap::from([(
            PkgName::parse("optional-child").expect("parse alias"),
            SnapshotDepRef::Link("../child".to_string()),
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
        |_| Err(io::Error::new(io::ErrorKind::PermissionDenied, "fixture denial")),
    )
    .expect_err("permission errors must abort the warm-slot probe");

    assert!(matches!(
        error,
        CreateVirtualStoreError::InspectOptionalDependency { error, .. }
            if error.kind() == io::ErrorKind::PermissionDenied
    ));
}
