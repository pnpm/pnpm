use super::GateWorkspace;
use pnpm_config::Config;
use std::fs;
use tempfile::tempdir;

#[test]
fn only_a_gate_with_other_discovery_results_skips_the_shared_projects() {
    let dir = tempdir().expect("create a temporary directory");
    let root = dir.path();
    fs::write(root.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    fs::create_dir_all(root.join("packages/a")).expect("create packages/a");
    fs::write(root.join("packages/a/package.json"), r#"{"name":"a"}"#)
        .expect("write packages/a/package.json");
    let mut config = Config::new();
    config.workspace_dir = Some(root.to_path_buf());
    config.modules_dir = root.join("packages/a/node_modules");
    let mut own_node_modules = config.clone();
    own_node_modules.modules_dir = root.join("packages/b/node_modules");
    let mut other_modules_dir = config.clone();
    other_modules_dir.modules_dir = root.join("packages/a/other_modules");
    let workspace = GateWorkspace::of(root, &config).expect("a workspace");

    let projects = workspace.projects(&config).expect("the discovered projects");
    assert_eq!(projects.len(), 1);
    assert!(workspace.projects(&own_node_modules).is_some());
    assert!(workspace.projects(&other_modules_dir).is_none());
}
