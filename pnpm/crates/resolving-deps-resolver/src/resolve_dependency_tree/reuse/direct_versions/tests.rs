use super::{
    DirectDepVersion, DirectDepVersions, cross_importer_changed_version, extend_parsed_versions,
    higher_direct_dep_version,
};
use pnpm_lockfile::{DeclaredRegistryPrefixes, PkgName, RegistryContext, SnapshotDepRef};
use std::collections::{BTreeMap, HashMap};

const DEFAULT: &str = "https://registry.example.test/";
const WORK: &str = "https://work.example.test/";

/// `public` declared for the default registry, `work` for another one.
fn registries() -> RegistryContext {
    RegistryContext {
        registries: HashMap::from([("default".to_string(), DEFAULT.to_string())]),
        registries_by_prefix: HashMap::from([
            ("public".to_string(), DEFAULT.to_string()),
            ("work".to_string(), WORK.to_string()),
        ]),
        declared_prefixes: DeclaredRegistryPrefixes::new([("public", DEFAULT), ("work", WORK)]),
        ..Default::default()
    }
}

fn recorded(versions: &[&str]) -> DirectDepVersions {
    let mut by_name = DirectDepVersions::default();
    let level = BTreeMap::from([(
        "react".to_string(),
        versions
            .iter()
            .map(ToString::to_string)
            .collect(),
    )]);
    extend_parsed_versions(&mut by_name, &level, &registries());
    by_name
}

fn pinned(version: &str) -> SnapshotDepRef {
    serde_json::from_value(serde_json::json!(version)).expect("parse dependency reference")
}

fn changed(cross_changed: &DirectDepVersions, version: &str) -> bool {
    let name: PkgName = "react".parse().expect("parse name");
    cross_importer_changed_version(cross_changed, &name, &pinned(version), &registries())
}

/// A child pin differs from another importer's changed direct dependency
/// only when that dependency comes from the pin's registry.
#[test]
fn a_pin_differs_only_from_a_changed_direct_version_of_its_registry() {
    let cross_changed = recorded(&["18.2.0", "work:18.2.0"]);
    assert!(changed(&cross_changed, "17.0.2"));
    assert!(changed(&cross_changed, "public:17.0.2"));
    assert!(changed(&cross_changed, "work:17.0.2"));
    assert!(!changed(&cross_changed, "work:18.2.0"));

    let other_registry_only = recorded(&["work:18.2.0"]);
    assert!(!changed(&other_registry_only, "17.0.2"));
}

/// A plain key and a key qualified by the prefix its registry declares name
/// one registry, so an older plain pin is refreshed onto a direct version
/// the re-keyed lockfile records under the prefix.
#[test]
fn a_plain_pin_is_refreshed_onto_a_qualified_direct_version_of_its_registry() {
    let direct = recorded(&["public:18.2.0", "work:18.3.0"]);
    let pin = DirectDepVersion::of("react", &"17.0.2".parse().expect("parse pin"), &registries())
        .expect("a registry version");
    let range = "^17.0.0 || ^18.0.0".parse().expect("parse range");
    assert_eq!(
        higher_direct_dep_version(Some(&direct), "react", &pin, &range).map(|v| v.to_string()),
        Some("18.2.0".to_string()),
    );
}

/// A resolved key suffix is recorded with the registry it names.
#[test]
fn records_the_registry_of_each_suffix() {
    let versions: Vec<(String, String)> =
        recorded(&["18.2.0", "public:18.2.1", "work:18.3.0"])["react"]
            .iter()
            .map(|direct| (direct.registry.clone(), direct.version.to_string()))
            .collect();
    assert_eq!(
        versions,
        [
            (DEFAULT.to_string(), "18.2.0".to_string()),
            (DEFAULT.to_string(), "18.2.1".to_string()),
            (WORK.to_string(), "18.3.0".to_string()),
        ],
    );
}
