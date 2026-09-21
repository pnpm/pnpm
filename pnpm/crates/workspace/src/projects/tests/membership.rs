use super::{
    super::{FindWorkspaceProjectsOpts, belongs_to_workspace, is_workspace_project_dir},
    make_project, make_yaml_project,
};
use pretty_assertions::assert_eq;
use std::{fs, path::Path};
use tempfile::TempDir;

/// The directories a walk of the fixture below could ever return, so the
/// agreement check covers the ones the patterns leave out as well.
const PROJECT_DIRS: &[&str] = &[
    ".",
    "packages/a",
    "packages/a/nested",
    "packages/b",
    "examples/example-1",
    "docs",
    "libs/yaml-only",
    "node_modules/dep",
    ".hidden/tool",
];

fn make_workspace() -> TempDir {
    let tmp = TempDir::new().unwrap();
    for dir in PROJECT_DIRS {
        if *dir == "libs/yaml-only" {
            make_yaml_project(tmp.path(), dir, "yaml-only");
        } else {
            make_project(tmp.path(), dir, &dir.replace('/', "-"));
        }
    }
    tmp
}

fn project_dirs(root: &Path, patterns: &[&str]) -> Vec<String> {
    let opts = FindWorkspaceProjectsOpts {
        patterns: Some(
            patterns
                .iter()
                .map(|pattern| (*pattern).to_string())
                .collect(),
        ),
    };
    let mut found: Vec<String> = super::super::find_workspace_projects(root, &opts)
        .unwrap()
        .iter()
        .map(|project| relative_posix(root, &project.root_dir))
        .collect();
    found.sort();
    found
}

fn relative_posix(root: &Path, dir: &Path) -> String {
    let relative = dir.strip_prefix(root).unwrap();
    if relative.as_os_str().is_empty() {
        ".".to_string()
    } else {
        relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }
}

#[test]
fn agrees_with_the_workspace_walk() {
    let tmp = make_workspace();
    let pattern_sets: &[&[&str]] = &[
        &["."],
        &["packages/*"],
        &["packages/**"],
        &["**"],
        &["**", "!examples/**"],
        &["**", "!/examples/**"],
        &["./packages/a"],
        &["packages/missing/../a"],
        &["packages/**", "libs/*"],
        &["!packages/**"],
    ];
    for patterns in pattern_sets {
        let selected = project_dirs(tmp.path(), patterns);
        let opts = FindWorkspaceProjectsOpts {
            patterns: Some(
                patterns
                    .iter()
                    .map(|pattern| (*pattern).to_string())
                    .collect(),
            ),
        };
        for dir in PROJECT_DIRS {
            let absolute =
                if *dir == "." { tmp.path().to_path_buf() } else { tmp.path().join(dir) };
            assert_eq!(
                (patterns, dir, is_workspace_project_dir(tmp.path(), &absolute, &opts).unwrap()),
                (patterns, dir, selected.contains(&(*dir).to_string())),
            );
        }
    }
}

#[test]
fn the_workspace_root_always_belongs_to_the_workspace() {
    let tmp = make_workspace();
    let patterns = ["packages/*".to_string()];
    assert!(belongs_to_workspace(tmp.path(), tmp.path(), Some(&patterns)).unwrap());
}

#[test]
fn a_directory_without_a_manifest_belongs_to_the_workspace() {
    let tmp = make_workspace();
    let src = tmp.path().join("packages/a/src");
    fs::create_dir_all(&src).unwrap();
    let patterns = ["packages/*".to_string()];
    assert!(belongs_to_workspace(tmp.path(), &src, Some(&patterns)).unwrap());
}

#[test]
fn an_excluded_project_does_not_belong_to_the_workspace() {
    let tmp = make_workspace();
    let patterns = ["packages/**".to_string(), "!examples/**".to_string()];
    assert!(
        !belongs_to_workspace(tmp.path(), &tmp.path().join("examples/example-1"), Some(&patterns))
            .unwrap(),
    );
}

#[test]
fn an_unlisted_project_does_not_belong_to_the_workspace() {
    let tmp = make_workspace();
    let patterns = ["packages/**".to_string()];
    assert!(!belongs_to_workspace(tmp.path(), &tmp.path().join("docs"), Some(&patterns)).unwrap());
}

#[test]
fn without_packages_only_the_workspace_root_belongs_to_the_workspace() {
    let tmp = make_workspace();
    assert!(belongs_to_workspace(tmp.path(), tmp.path(), None).unwrap());
    assert!(!belongs_to_workspace(tmp.path(), &tmp.path().join("packages/a"), None).unwrap());
}
