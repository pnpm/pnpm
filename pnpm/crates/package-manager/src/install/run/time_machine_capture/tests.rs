use super::{InstallScope, project_dirs_to_capture};
use pnpm_package_manifest::PackageManifest;
use serde_json::json;
use std::{collections::HashSet, path::PathBuf};

#[test]
fn captures_unselected_workspace_projects_reached_by_workspace_dependencies() {
    let selected_dir = PathBuf::from("/workspace/packages/selected");
    let linked_dir = PathBuf::from("/workspace/packages/linked");
    let workspace_dir = PathBuf::from("/workspace/packages/workspace");
    let unrelated_dir = PathBuf::from("/workspace/packages/unrelated");
    let selected_manifest = PackageManifest::from_value(
        selected_dir.join("package.json"),
        json!({ "name": "selected", "dependencies": { "linked": "link:../linked" } }),
    );
    let linked_manifest = PackageManifest::from_value(
        linked_dir.join("package.json"),
        json!({ "name": "linked", "peerDependencies": { "workspace": "workspace:*" } }),
    );
    let workspace_manifest = PackageManifest::from_value(
        workspace_dir.join("package.json"),
        json!({ "name": "workspace", "version": "1.0.0" }),
    );
    let unrelated_manifest = PackageManifest::from_value(
        unrelated_dir.join("package.json"),
        json!({ "name": "unrelated", "version": "1.0.0" }),
    );
    let scope = InstallScope {
        project_manifests: vec![
            (selected_dir.clone(), &selected_manifest),
            (linked_dir.clone(), &linked_manifest),
            (workspace_dir.clone(), &workspace_manifest),
            (unrelated_dir, &unrelated_manifest),
        ],
        importers: crate::install::run::workspace::ImporterSelection {
            real_importer_ids: HashSet::new(),
            filtered_install: true,
            requested_importer_ids: None,
        },
        prune_stale_importers: false,
    };

    assert_eq!(
        project_dirs_to_capture(&scope, Some(&HashSet::from([selected_dir.clone()])), true),
        vec![selected_dir, linked_dir, workspace_dir],
    );
}
