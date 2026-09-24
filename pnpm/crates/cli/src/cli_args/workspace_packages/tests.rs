use std::path::PathBuf;

use pnpm_package_manifest::PackageManifest;
use pnpm_workspace::Project;
use serde_json::json;

use super::build_workspace_package_manifest_map;

fn project(root_dir: &str, name: &str, version: Option<&str>) -> Project {
    let root_dir = PathBuf::from(root_dir);
    let mut manifest = json!({ "name": name });
    if let Some(version) = version {
        manifest["version"] = json!(version);
    }
    Project {
        root_dir: root_dir.clone(),
        manifest: PackageManifest::from_value(root_dir.join("package.json"), manifest),
        dependency_manifest: None,
    }
}

#[test]
fn versioned_manifest_replaces_an_earlier_name_only_manifest() {
    let map = build_workspace_package_manifest_map(&[
        project("/workspace/incomplete", "pkg-b", None),
        project("/workspace/complete", "pkg-b", Some("2.0.0")),
    ]);

    assert_eq!(map["pkg-b"].version, "2.0.0");
}

#[test]
fn first_complete_manifest_wins_over_a_later_complete_manifest() {
    let map = build_workspace_package_manifest_map(&[
        project("/workspace/first", "pkg-b", Some("1.0.0")),
        project("/workspace/second", "pkg-b", Some("2.0.0")),
    ]);

    assert_eq!(map["pkg-b"].version, "1.0.0");
}
