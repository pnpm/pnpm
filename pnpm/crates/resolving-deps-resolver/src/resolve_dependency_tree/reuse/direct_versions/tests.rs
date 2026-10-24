use super::cross_importer_changed_version;
use pnpm_lockfile::{PkgName, SnapshotDepRef};

/// A child pinned under a registry-qualified key differs from another
/// importer's changed direct dependency the way a plain pin does.
#[test]
fn a_registry_qualified_pin_differs_from_a_changed_direct_version() {
    let name: PkgName = "react".parse().expect("parse name");
    let cross_changed = super::DirectDepVersions::from_iter([(
        "react".to_string(),
        vec!["18.2.0".parse().expect("parse version")],
    )]);
    let pinned = |version: &str| -> SnapshotDepRef {
        serde_json::from_value(serde_json::json!(version)).expect("parse dependency reference")
    };

    assert!(cross_importer_changed_version(&cross_changed, &name, &pinned("17.0.2")));
    assert!(cross_importer_changed_version(&cross_changed, &name, &pinned("work:17.0.2")));
    assert!(!cross_importer_changed_version(&cross_changed, &name, &pinned("work:18.2.0")));
}
