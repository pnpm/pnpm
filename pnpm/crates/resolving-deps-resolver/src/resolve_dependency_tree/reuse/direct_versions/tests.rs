use super::{cross_importer_changed_version, extend_parsed_versions};
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

/// A direct dependency from a registry that declares a prefix resolves to a
/// registry-qualified key suffix, whose version is recorded like a plain one.
#[test]
fn records_the_version_of_a_registry_qualified_suffix() {
    let mut by_name = super::DirectDepVersions::default();
    let level = std::collections::BTreeMap::from([(
        "react".to_string(),
        vec!["18.2.0".to_string(), "work:18.3.0".to_string()],
    )]);
    extend_parsed_versions(&mut by_name, &level);
    let versions: Vec<String> = by_name["react"]
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(versions, ["18.2.0", "18.3.0"]);
}
