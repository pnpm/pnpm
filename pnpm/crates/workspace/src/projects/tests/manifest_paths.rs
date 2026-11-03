use super::{
    super::{ProjectManifestPaths, find_workspace_project_manifests},
    FindWorkspaceProjectsOpts, find_workspace_projects, make_project,
};
use pretty_assertions::assert_eq;
use std::{fs, path::PathBuf};
use tempfile::TempDir;

fn opts(patterns: &[&str]) -> FindWorkspaceProjectsOpts {
    FindWorkspaceProjectsOpts {
        patterns: Some(
            patterns
                .iter()
                .map(|pattern| (*pattern).to_string())
                .collect(),
        ),
        ignored_directories: vec![PathBuf::from("node_modules")],
    }
}

#[test]
fn lists_the_project_directories_discovery_reads() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    make_project(root, ".", "root");
    make_project(root, "packages/a", "a");
    make_project(root, "packages/b", "b");
    make_project(root, "other/c", "c");
    fs::create_dir_all(root.join("packages/no-manifest")).unwrap();
    let opts = opts(&["packages/*", "other/c"]);

    let listed = find_workspace_project_manifests(root, &opts).unwrap();
    let read = find_workspace_projects(root, &opts).unwrap();

    let listed_dirs = listed
        .iter()
        .map(|project| project.root_dir.clone())
        .collect::<Vec<_>>();
    let read_dirs = read
        .iter()
        .map(|project| project.root_dir.clone())
        .collect::<Vec<_>>();
    let mut projects_and_empty = read_dirs.clone();
    projects_and_empty.push(root.join("packages/no-manifest"));
    projects_and_empty.sort();
    assert_eq!(listed_dirs, projects_and_empty, "a `packages/*` directory is listed unchecked");
    assert_eq!(
        read_dirs,
        [
            root.to_path_buf(),
            root.join("other/c"),
            root.join("packages/a"),
            root.join("packages/b"),
        ],
    );
}

/// Listing never opens a manifest, so one that cannot be parsed is still
/// listed, ahead of the candidates behind it.
#[test]
fn lists_a_manifest_it_cannot_parse_first() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let project = root.join("a");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("package.yaml"), "name: a\n").unwrap();
    fs::write(project.join("package.json"), "{ not json").unwrap();
    let opts = opts(&["a"]);

    let listed = find_workspace_project_manifests(root, &opts).unwrap();

    assert_eq!(
        listed,
        [ProjectManifestPaths {
            root_dir: project.clone(),
            candidates: vec![project.join("package.json"), project.join("package.yaml")],
        }],
    );
    assert!(find_workspace_projects(root, &opts).is_err());
}
