use super::build_workspace_package_manifest_map;
use pnpm_package_manifest::PackageManifest;
use serde_json::json;
use std::path::PathBuf;

#[test]
fn build_workspace_package_manifest_map_preserves_first_encountered_name() {
    let first = PackageManifest::from_value(
        PathBuf::from("a/package.json"),
        json!({ "name": "dup-pkg", "version": "1.0.0" }),
    );
    let second = PackageManifest::from_value(
        PathBuf::from("b/package.json"),
        json!({ "name": "dup-pkg", "version": "2.0.0" }),
    );
    let projects = [
        pnpm_workspace::Project {
            root_dir: PathBuf::from("a"),
            manifest: first,
            dependency_manifest: None,
        },
        pnpm_workspace::Project {
            root_dir: PathBuf::from("b"),
            manifest: second,
            dependency_manifest: None,
        },
    ];

    let map = build_workspace_package_manifest_map(&projects);
    assert_eq!(map["dup-pkg"].version, "1.0.0");
}
