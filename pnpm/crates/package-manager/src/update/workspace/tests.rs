use super::{UpdateError, workspace_link_targets};
use crate::update::selectors::parse_update_param;
use pnpm_config::Config;
use pnpm_package_manifest::DependencyGroup;
use pnpm_resolving_resolver_base::{
    WorkspacePackage, WorkspacePackages, WorkspacePackagesByVersion,
};
use std::path::PathBuf;

#[test]
fn workspace_link_targets_errors_on_missing_workspace_package_when_not_interactive() {
    let direct = vec![("external".to_string(), DependencyGroup::Prod, "^1.0.0".to_string())];
    let selectors = vec![parse_update_param("external")];
    let workspace_packages = WorkspacePackages::default();
    let config = Config::new();

    let err = workspace_link_targets(&selectors, &direct, &workspace_packages, &config, false)
        .expect_err("should fail when package is not in workspace");
    assert!(matches!(err, UpdateError::WorkspacePackageNotFound(name) if name == "external"));
}

#[test]
fn workspace_link_targets_skips_external_package_when_interactive() {
    let direct = vec![
        ("external".to_string(), DependencyGroup::Prod, "^1.0.0".to_string()),
        ("workspace-pkg".to_string(), DependencyGroup::Prod, "^1.0.0".to_string()),
    ];
    let selectors = vec![parse_update_param("external"), parse_update_param("workspace-pkg")];
    let mut workspace_versions = WorkspacePackagesByVersion::new();
    workspace_versions.insert(
        "1.0.0".to_string(),
        WorkspacePackage {
            root_dir: PathBuf::from("/workspace/workspace-pkg"),
            manifest: serde_json::json!({ "name": "workspace-pkg", "version": "1.0.0" }),
        },
    );
    let mut workspace_packages = WorkspacePackages::default();
    workspace_packages.insert("workspace-pkg".to_string(), workspace_versions);
    let config = Config::new();

    let targets = workspace_link_targets(&selectors, &direct, &workspace_packages, &config, true)
        .expect("should succeed in interactive mode");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].name, "workspace-pkg");
}
