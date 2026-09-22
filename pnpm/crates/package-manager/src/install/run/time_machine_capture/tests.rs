use super::{InstallScope, project_dirs_to_capture};
use crate::install::run::workspace::ImporterSelection;
use pnpm_package_manifest::PackageManifest;
use serde_json::json;
use std::{collections::HashSet, path::PathBuf};

#[test]
fn captures_unselected_workspace_projects_reached_by_link_dependencies() {
    let selected_dir = PathBuf::from("/workspace/packages/selected");
    let linked_dir = PathBuf::from("/workspace/packages/linked");
    let selected_manifest = PackageManifest::from_value(
        selected_dir.join("package.json"),
        json!({ "name": "selected", "dependencies": { "linked": "link:../linked" } }),
    );
    let linked_manifest =
        PackageManifest::from_value(linked_dir.join("package.json"), json!({ "name": "linked" }));
    let scope = InstallScope {
        project_manifests: vec![
            (selected_dir.clone(), &selected_manifest),
            (linked_dir.clone(), &linked_manifest),
        ],
        importers: ImporterSelection {
            real_importer_ids: HashSet::from(["packages/selected".to_string()]),
            filtered_install: true,
            requested_importer_ids: Some(HashSet::from(["packages/selected".to_string()])),
        },
        prune_stale_importers: false,
    };

    assert_eq!(project_dirs_to_capture(&scope), vec![selected_dir, linked_dir]);
}
