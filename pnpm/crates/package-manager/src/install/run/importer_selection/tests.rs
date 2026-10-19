use super::{HashSet, ImporterSelection, PackageManifest, Path, PathBuf};

#[test]
fn a_filter_matching_every_on_disk_project_still_sets_requested_importer_ids() {
    let workspace_root = Path::new("/ws");
    let dir_a = PathBuf::from("/ws/packages/a");
    let manifest_a = PackageManifest::from_value(
        dir_a.join("package.json"),
        serde_json::json!({ "name": "a", "version": "1.0.0" }),
    );
    let project_manifests = vec![(dir_a.clone(), &manifest_a)];
    let project_dependencies = indexmap::IndexMap::new();
    let selected_dirs = HashSet::from([dir_a.clone()]);
    let install_dirs = HashSet::from([dir_a]);
    let selection = crate::WorkspaceInstallSelection {
        all_projects: &[],
        project_dependencies: &project_dependencies,
        ordered_dirs: &[],
        selected_dirs: &selected_dirs,
        edited_dirs: None,
        install_dirs: &install_dirs,
        active_manifest_is_standin: false,
        workspace_cycles: crate::PrecomputedWorkspaceCycles::Unknown,
    };
    let result =
        ImporterSelection::select(Some(&selection), true, workspace_root, &project_manifests);
    assert!(!result.filtered_install);
    assert_eq!(result.requested_importer_ids, Some(HashSet::from([String::from("packages/a")])));
}
