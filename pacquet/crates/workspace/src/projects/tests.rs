use super::{FindWorkspaceProjectsOpts, find_workspace_projects};
use pretty_assertions::assert_eq;
use std::fs;
use tempfile::TempDir;

fn make_project(root: &std::path::Path, rel: &str, name: &str) {
    let dir = root.join(rel);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("package.json"), format!(r#"{{"name": "{name}", "version": "0.0.1"}}"#))
        .unwrap();
}

#[test]
fn expands_packages_glob() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "packages/alpha", "alpha");
    make_project(tmp.path(), "packages/beta", "beta");

    let projects = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts { patterns: Some(vec!["packages/*".to_string()]) },
    )
    .unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    // Sorted lex by rootDir → root then alpha then beta.
    assert_eq!(names, vec!["root".to_string(), "alpha".to_string(), "beta".to_string()]);
}

#[test]
fn always_includes_workspace_root() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "apps/web", "web");

    // Patterns deliberately do NOT cover the root. Upstream still
    // surfaces it (https://github.com/pnpm/pnpm/issues/1986).
    let projects = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts { patterns: Some(vec!["apps/*".to_string()]) },
    )
    .unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, vec!["root".to_string(), "web".to_string()]);
}

#[test]
fn filters_node_modules() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "node_modules/foo", "foo");
    make_project(tmp.path(), "packages/real", "real");

    let projects = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts { patterns: Some(vec!["**".to_string()]) },
    )
    .unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    assert!(
        !names.contains(&"foo".to_string()),
        "node_modules contents must not surface as workspace projects: {names:?}",
    );
    assert!(
        names.contains(&"real".to_string()),
        "expected the `real` project to be enumerated; got {names:?}",
    );
}

#[test]
fn dedupes_overlapping_patterns() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "packages/alpha", "alpha");

    // Two patterns that both match `packages/alpha/package.json` should
    // produce exactly one entry.
    let projects = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts {
            patterns: Some(vec!["packages/*".to_string(), "**".to_string()]),
        },
    )
    .unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, vec!["root".to_string(), "alpha".to_string()]);
}

#[test]
fn default_patterns_when_packages_omitted() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "apps/web", "web");

    let projects =
        find_workspace_projects(tmp.path(), &FindWorkspaceProjectsOpts { patterns: None }).unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    // `.` + `**` enumerates everything.
    assert_eq!(names, vec!["root".to_string(), "web".to_string()]);
}

/// `packages: []` (explicit empty array) is *not* the same as
/// omitted: it means "enumerate only the workspace root project,"
/// matching upstream's `opts.patterns ?? defaults` where `[]` is a
/// truthy value that survives the nullish-coalesce. Without this,
/// `packages: []` would silently fall back to `['.', '**']` and
/// recurse through the whole tree.
#[test]
fn empty_patterns_array_enumerates_root_only() {
    let tmp = TempDir::new().unwrap();
    make_project(tmp.path(), ".", "root");
    make_project(tmp.path(), "apps/web", "web");

    let projects = find_workspace_projects(
        tmp.path(),
        &FindWorkspaceProjectsOpts { patterns: Some(Vec::new()) },
    )
    .unwrap();

    let names: Vec<String> = projects
        .iter()
        .map(|project| project.manifest.value().get("name").unwrap().as_str().unwrap().to_string())
        .collect();
    // Only the workspace root surfaces — `web` is not enumerated.
    assert_eq!(names, vec!["root".to_string()]);
}
