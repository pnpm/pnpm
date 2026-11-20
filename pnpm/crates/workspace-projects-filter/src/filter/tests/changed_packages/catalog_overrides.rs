use super::{
    TestPkg,
    commit_all,
    diff_selector,
    graph_project,
    init_repo,
    selected,
};
use crate::filter::FilterWorkspaceProjectsOptions;
use indexmap::IndexMap;
use pnpm_workspace_projects_graph::{
    ProjectGraph,
    ProjectGraphNode,
};
use std::{
    collections::HashMap,
    fs,
    path::Path,
};
use tempfile::TempDir;

#[test]
fn selects_projects_by_the_given_project_dependencies() {
    let workspace = TempDir::new().expect("create tempdir");
    let workspace_dir = workspace.path();
    init_repo(workspace_dir);

    let app_dir = workspace_dir.join("packages/app");
    let other_dir = workspace_dir.join("packages/other");
    fs::create_dir_all(&app_dir).expect("create app");
    fs::create_dir_all(&other_dir).expect("create other");
    let write_manifest = |lib: &str| {
        fs::write(
            workspace_dir.join("pnpm-workspace.yaml"),
            format!("packages:\n  - 'packages/*'\ncatalogs:\n  local:\n    lib: {lib}\n"),
        )
        .expect("write workspace manifest");
    };
    write_manifest("^1.0.0");
    commit_all(workspace_dir);
    write_manifest("^1.1.0");

    let mut graph: ProjectGraph<TestPkg> = IndexMap::new();
    for (dir, name, deps) in
        [(&app_dir, "app", [("lib", "^1.0.0")]), (&other_dir, "other", [("lib", "catalog:local")])]
    {
        graph.insert(
            dir.clone(),
            ProjectGraphNode {
                package: graph_project(&dir.to_string_lossy(), name, &deps),
                dependencies: Vec::new(),
            },
        );
    }
    let path_of = |dir: &Path| dir.to_string_lossy().into_owned();
    let declared = FilterWorkspaceProjectsOptions {
        workspace_dir: workspace_dir.to_path_buf(),
        ..Default::default()
    };
    assert_eq!(selected(&graph, &[diff_selector("HEAD")], &declared), [path_of(&other_dir)]);

    let overridden = FilterWorkspaceProjectsOptions {
        project_dependencies: Some(HashMap::from([
            (app_dir.clone(), vec![("lib".to_string(), "catalog:local".to_string())]),
            (other_dir, vec![("lib".to_string(), "^2.0.0".to_string())]),
        ])),
        ..declared
    };
    assert_eq!(selected(&graph, &[diff_selector("HEAD")], &overridden), [path_of(&app_dir)]);
}
