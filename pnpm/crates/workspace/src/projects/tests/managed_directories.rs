use super::{
    super::{FindWorkspaceProjectsOpts, find_workspace_projects, is_workspace_project_dir},
    make_project,
};
use pretty_assertions::assert_eq;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

fn find_sorted_names(
    root: &Path,
    patterns: &[&str],
    ignored_directories: Vec<PathBuf>,
) -> Vec<String> {
    let opts = FindWorkspaceProjectsOpts {
        patterns: Some(
            patterns
                .iter()
                .map(|pattern| (*pattern).to_string())
                .collect(),
        ),
        ignored_directories,
    };
    let mut names: Vec<String> = find_workspace_projects(root, &opts)
        .unwrap()
        .iter()
        .map(|project| {
            project.manifest
                .value()
                .get("name")
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn skips_pnpm_managed_directories() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "packages/real", "real");
    make_project(tmp.path(), "store/v11/python-envs/tool", "tool");
    make_project(tmp.path(), "store/v11/files/pkg", "pkg");

    assert_eq!(find_sorted_names(tmp.path(), &["**"], Vec::new()), ["pkg", "real", "root", "tool"]);
    // Relative, the way `storeDir: store` reaches the option.
    assert_eq!(
        find_sorted_names(tmp.path(), &["**"], vec![PathBuf::from("store")]),
        ["real", "root"],
    );
}

#[test]
fn skips_a_managed_directory_a_literal_pattern_names() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "store/tool", "tool");
    make_project(tmp.path(), "store/children/pkg", "pkg");

    assert_eq!(
        find_sorted_names(
            tmp.path(),
            &["store/tool", "store/children/*"],
            vec![tmp.path().join("store")]
        ),
        ["root"],
    );
}

#[test]
fn workspace_nested_inside_managed_directory_keeps_its_projects() {
    // The pipeline watch agent checks repositories out under the state
    // directory.
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), "xdg-state/pnpm/pipeline/agent/checkout/demo", "root");
    make_project(tmp.path(), "xdg-state/pnpm/pipeline/agent/checkout/demo/pkg", "pkg");
    let checkout = tmp.path().join("xdg-state/pnpm/pipeline/agent/checkout/demo");

    assert_eq!(
        find_sorted_names(&checkout, &["pkg"], vec![tmp.path().join("xdg-state/pnpm")]),
        ["pkg", "root"],
    );
}

#[test]
fn managed_directory_equal_to_workspace_root_hides_nothing() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "pkg", "pkg");

    assert_eq!(
        find_sorted_names(tmp.path(), &["pkg"], vec![tmp.path().to_path_buf()]),
        ["pkg", "root"],
    );
}

#[test]
fn skips_a_managed_directory_outside_the_workspace_root() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), "workspace", "workspace-root");
    make_project(tmp.path(), "workspace/pkg", "real");
    make_project(tmp.path(), "managed/tool", "tool");
    let workspace = tmp.path().join("workspace");

    assert_eq!(
        find_sorted_names(
            &workspace,
            &["pkg", "../managed/*", "../**"],
            vec![PathBuf::from("../managed")]
        ),
        ["real", "workspace-root"],
    );
}

#[test]
fn skips_a_managed_directory_when_the_root_is_spelled_with_dot_dot() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("detour")).unwrap();
    make_project(tmp.path(), "workspace", "workspace-root");
    make_project(tmp.path(), "workspace/store/tool", "store-tool");
    make_project(tmp.path(), "managed/tool", "tool");
    let workspace = tmp.path().join("detour/../workspace");

    assert_eq!(
        find_sorted_names(
            &workspace,
            &["store/tool", "../managed/*"],
            vec![tmp.path().join("workspace/store"), tmp.path().join("managed"),]
        ),
        ["workspace-root"],
    );
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn skips_a_managed_directory_configured_with_different_casing() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "store/tool", "tool");
    make_project(tmp.path(), "store/children/pkg", "pkg");

    assert_eq!(
        find_sorted_names(
            tmp.path(),
            &["**", "store/tool", "store/children/*"],
            vec![PathBuf::from("STORE")]
        ),
        ["root"],
    );
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn workspace_inside_a_managed_directory_configured_with_different_casing_keeps_its_projects() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), "state/checkout", "root");
    make_project(tmp.path(), "state/checkout/pkg", "pkg");

    assert_eq!(
        find_sorted_names(
            &tmp.path().join("state/checkout"),
            &["pkg"],
            vec![tmp.path().join("STATE")]
        ),
        ["pkg", "root"],
    );
}

#[cfg(target_os = "linux")]
#[test]
fn keeps_a_directory_that_differs_from_a_managed_one_only_in_case() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "store/tool", "tool");
    make_project(tmp.path(), "Store/pkg", "pkg");

    assert_eq!(
        find_sorted_names(tmp.path(), &["**"], vec![PathBuf::from("store")]),
        ["pkg", "root"],
    );
}

#[test]
fn a_managed_directory_is_never_a_workspace_project_dir() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "store/tool", "tool");
    make_project(tmp.path(), "packages/real", "real");
    let opts = FindWorkspaceProjectsOpts {
        patterns: Some(vec!["**".to_string()]),
        ignored_directories: vec![PathBuf::from("store")],
    };

    assert!(!is_workspace_project_dir(tmp.path(), &tmp.path().join("store/tool"), &opts).unwrap());
    assert!(
        is_workspace_project_dir(tmp.path(), &tmp.path().join("packages/real"), &opts).unwrap(),
    );
}
