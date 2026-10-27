use super::registry_package_ids;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn a_semver_version_names_one_registry_package() {
    let manifest = json!({ "name": "acme", "version": "1.0.0" });
    assert_eq!(registry_package_ids(Some(&manifest)), ["acme@1.0.0"]);
}

#[test]
fn a_v_prefixed_version_also_names_the_version_semver_writes() {
    let manifest = json!({ "name": "acme", "version": "v1.0.0" });
    assert_eq!(registry_package_ids(Some(&manifest)), ["acme@v1.0.0", "acme@1.0.0"]);
}

#[test]
fn a_manifest_without_a_version_names_no_registry_package() {
    let manifest = json!({ "name": "acme" });
    assert_eq!(registry_package_ids(Some(&manifest)), Vec::<String>::new());
}
