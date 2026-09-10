use std::path::PathBuf;

use pnpm_package_manifest::PackageManifest;
use serde_json::json;

use super::validate_peer_dependencies;

fn manifest(value: serde_json::Value) -> PackageManifest {
    PackageManifest::from_value(PathBuf::from("package.json"), value)
}

#[test]
fn a_manifest_without_peer_dependencies_passes() {
    let manifest = manifest(json!({ "name": "proj", "dependencies": { "foo": "1.0.0" } }));
    assert!(validate_peer_dependencies(&manifest, ".").is_ok());
}

#[test]
fn every_acceptable_peer_spec_shape_passes() {
    let manifest = manifest(json!({
        "name": "proj",
        "peerDependencies": {
            "range": "^1.0.0",
            "union": ">=1.2.3 || ^3.2.1",
            "workspace": "workspace:^",
            "catalog": "catalog:",
            "named-registry": "work:5.x.x",
            "alias": "npm:bar@^5",
            "file": "file:../foo",
            "git": "git+https://example.com/foo.git",
        },
    }));
    assert!(validate_peer_dependencies(&manifest, ".").is_ok());
}

#[test]
fn a_bare_name_at_version_typo_is_rejected() {
    let manifest = manifest(json!({ "name": "proj", "peerDependencies": { "bar": "bar@1.2.3" } }));
    let err = validate_peer_dependencies(&manifest, ".").expect_err("the typo must be rejected");
    assert_eq!(err.project_id, "proj");
    assert_eq!(err.dep_name, "bar");
    assert_eq!(err.value, "bar@1.2.3");
    assert_eq!(
        err.to_string(),
        "The peerDependencies field named 'bar' of package 'proj' has an invalid value: 'bar@1.2.3'",
    );
}

#[test]
fn a_dist_tag_is_rejected() {
    let manifest = manifest(json!({ "name": "proj", "peerDependencies": { "bar": "latest" } }));
    let err = validate_peer_dependencies(&manifest, ".").expect_err("a dist tag is not a range");
    assert_eq!(err.value, "latest");
}

#[test]
fn a_nameless_manifest_is_named_by_its_importer_id() {
    let manifest = manifest(json!({ "peerDependencies": { "bar": "bar@1.2.3" } }));
    let err = validate_peer_dependencies(&manifest, "packages/app").expect_err("still rejected");
    assert_eq!(err.project_id, "packages/app");
}

#[test]
fn a_non_string_value_is_left_to_the_manifest_parser() {
    let manifest = manifest(json!({ "name": "proj", "peerDependencies": { "bar": 1 } }));
    assert!(validate_peer_dependencies(&manifest, ".").is_ok());
}
