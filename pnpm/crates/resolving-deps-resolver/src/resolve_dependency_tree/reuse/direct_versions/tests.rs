use super::{cross_importer_changed_version, extend_parsed_versions};
use pnpm_lockfile::{PkgName, SnapshotDepRef};

/// A child pin differs from another importer's changed direct dependency
/// only when that dependency comes from the pin's registry.
#[test]
fn a_registry_qualified_pin_differs_from_a_changed_direct_version() {
    let name: PkgName = "react".parse().expect("parse name");
    let cross_changed = super::DirectDepVersions::from_iter([(
        "react".to_string(),
        vec![
            super::DirectDepVersion {
                key_prefix: None,
                version: "18.2.0".parse().expect("parse version"),
            },
            super::DirectDepVersion {
                key_prefix: Some("work".to_string()),
                version: "18.2.0".parse().expect("parse version"),
            },
        ],
    )]);
    let registries = pnpm_lockfile::RegistryContext::default();
    let pinned = |version: &str| -> SnapshotDepRef {
        serde_json::from_value(serde_json::json!(version)).expect("parse dependency reference")
    };

    assert!(cross_importer_changed_version(&cross_changed, &name, &pinned("17.0.2"), &registries));
    assert!(cross_importer_changed_version(
        &cross_changed,
        &name,
        &pinned("work:17.0.2"),
        &registries
    ));
    assert!(!cross_importer_changed_version(
        &cross_changed,
        &name,
        &pinned("work:18.2.0"),
        &registries
    ));
}

/// A direct dependency from a registry that declares a prefix resolves to a
/// registry-qualified key suffix, whose version is recorded with that prefix.
#[test]
fn records_the_version_of_a_registry_qualified_suffix() {
    let mut by_name = super::DirectDepVersions::default();
    let level = std::collections::BTreeMap::from([(
        "react".to_string(),
        vec!["18.2.0".to_string(), "work:18.3.0".to_string()],
    )]);
    extend_parsed_versions(&mut by_name, &level, &pnpm_lockfile::RegistryContext::default());
    let versions: Vec<(Option<&str>, String)> = by_name["react"]
        .iter()
        .map(|direct| (direct.key_prefix.as_deref(), direct.version.to_string()))
        .collect();
    assert_eq!(versions, [(None, "18.2.0".to_string()), (Some("work"), "18.3.0".to_string())]);
}
