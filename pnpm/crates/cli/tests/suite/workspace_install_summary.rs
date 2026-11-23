//! The packages-diff summary of an install in a workspace of several
//! projects: changes merged across projects, attributed to the projects
//! that made them, and a project's first install kept out of the list.

use crate::_utils::is_summary_change_line;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{
    AddMockedRegistry,
    CommandTempCwd,
};
use pretty_assertions::assert_eq;
use std::{
    fs,
    path::Path,
    process::Command,
};

fn pacquet_at(dir: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(dir)
}

fn write_project(dir: &Path, name: &str, dependencies: &serde_json::Value) {
    fs::create_dir_all(dir).expect("create the project directory");
    let manifest = serde_json::json!({ "name": name, "dependencies": dependencies });
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
}

/// A workspace of the root and `projects`, each without dependencies.
fn workspace_of(projects: &[&str]) -> CommandTempCwd<AddMockedRegistry> {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    let workspace_yaml_path = fixture.workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");
    write_project(&fixture.workspace, "root", &serde_json::json!({}));
    for project in projects {
        write_project(
            &fixture.workspace.join("packages").join(project),
            project,
            &serde_json::json!({}),
        );
    }
    fixture
}

/// The summary's lines, from the first dependency-type header to the end
/// of the last change.
fn install_summary(dir: &Path) -> String {
    let assert = pacquet_at(dir)
        .with_arg("install")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    eprintln!("STDOUT:\n{stdout}");
    let is_summary_line =
        |line: &&str| line.ends_with("ependencies:") || is_summary_change_line(line);
    stdout
        .lines()
        .filter(is_summary_line)
        .collect::<Vec<_>>()
        .join("\n")
}

// Every version below is its package's latest, so no line carries an
// "is available" note and no first-install line is listed for being
// outdated.

#[test]
fn first_workspace_install_lists_only_deprecated_dependencies() {
    let fixture = workspace_of(&["a", "b"]);
    let workspace = &fixture.workspace;
    let foo = serde_json::json!({ "@pnpm.e2e/foo": "100.1.0" });
    write_project(workspace, "root", &foo);
    write_project(
        &workspace.join("packages/a"),
        "a",
        &serde_json::json!({ "@pnpm.e2e/deprecated": "1.0.0", "@pnpm.e2e/bar": "100.1.0" }),
    );
    write_project(&workspace.join("packages/b"), "b", &foo);

    assert_eq!(
        install_summary(workspace),
        "dependencies:\n+ @pnpm.e2e/deprecated 1.0.0 deprecated",
    );

    drop(fixture);
}

#[test]
fn workspace_install_attributes_changes_to_projects() {
    let fixture = workspace_of(&["a", "b", "c"]);
    let workspace = &fixture.workspace;
    let foo = serde_json::json!({ "@pnpm.e2e/foo": "100.1.0" });
    let bar = serde_json::json!({ "@pnpm.e2e/bar": "100.1.0" });
    let foo_and_bar = serde_json::json!({ "@pnpm.e2e/foo": "100.1.0", "@pnpm.e2e/bar": "100.1.0" });
    write_project(workspace, "root", &foo);
    for project in ["a", "b", "c"] {
        write_project(&workspace.join("packages").join(project), project, &foo);
    }
    assert_eq!(install_summary(workspace), "");

    write_project(workspace, "root", &bar);
    for project in ["a", "b"] {
        write_project(&workspace.join("packages").join(project), project, &foo_and_bar);
    }
    write_project(&workspace.join("packages/new"), "new", &bar);

    assert_eq!(
        install_summary(workspace),
        "dependencies:\n\
         + @pnpm.e2e/bar 100.1.0  in 3 projects\n\
         - @pnpm.e2e/foo 100.1.0",
    );

    drop(fixture);
}

#[test]
fn workspace_update_is_one_line_per_change() {
    let fixture = workspace_of(&["a", "b"]);
    let workspace = &fixture.workspace;
    let old_foo = serde_json::json!({ "@pnpm.e2e/foo": "100.0.0" });
    let new_foo = serde_json::json!({ "@pnpm.e2e/foo": "100.1.0" });
    write_project(workspace, "root", &serde_json::json!({}));
    for project in ["a", "b"] {
        write_project(&workspace.join("packages").join(project), project, &old_foo);
    }
    install_summary(workspace);

    for project in ["a", "b"] {
        write_project(&workspace.join("packages").join(project), project, &new_foo);
    }

    assert_eq!(install_summary(workspace), "dependencies:\n↑ @pnpm.e2e/foo 100.0.0 → 100.1.0");

    drop(fixture);
}
