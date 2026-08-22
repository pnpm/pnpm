use assert_cmd::prelude::*;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::{fs, path::Path, process::Command};

fn write_cyclic_workspace(workspace: &Path, extra_settings: &str) {
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("packages:\n  - packages/*\n{extra_settings}"),
    )
    .expect("write workspace manifest");
    for (name, dependency) in [("project-1", "project-2"), ("project-2", "project-1")] {
        let dir = workspace.join("packages").join(name);
        fs::create_dir_all(&dir).expect("create project dir");
        fs::write(
            dir.join("package.json"),
            json!({
                "name": name,
                "version": "1.0.0",
                "dependencies": { dependency: "workspace:*" },
            })
            .to_string(),
        )
        .expect("write package.json");
    }
}

/// The two dispatches reach the report from different sides: a
/// `--recursive` install reports over the selection the CLI resolved, a
/// plain one over the whole workspace from inside the installer.
fn recursive_install(workspace: &Path) -> Command {
    install_command(workspace, true)
}

fn install_command(workspace: &Path, recursive: bool) -> Command {
    let mut install = Command::cargo_bin("pnpm").unwrap();
    install.current_dir(workspace);
    if recursive {
        install.arg("--recursive");
    }
    install.arg("install");
    install
}

const CYCLE_MESSAGE: &str = "There are cyclic workspace dependencies";

#[test]
fn a_recursive_install_warns_about_cyclic_workspace_dependencies() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_cyclic_workspace(&workspace, "");

    let output = recursive_install(&workspace).assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains(&format!("[WARN] {CYCLE_MESSAGE}")), "{stdout}");
    assert!(stdout.contains("project-1"), "{stdout}");
    assert!(stdout.contains("project-2"), "{stdout}");
}

#[test]
fn ignore_workspace_cycles_silences_the_warning() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_cyclic_workspace(&workspace, "ignoreWorkspaceCycles: true\n");

    let output = recursive_install(&workspace).assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(!stdout.contains(CYCLE_MESSAGE), "{stdout}");
}

#[test]
fn disallow_workspace_cycles_makes_the_cycle_an_error() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_cyclic_workspace(&workspace, "disallowWorkspaceCycles: true\n");

    let output = recursive_install(&workspace).assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_DISALLOW_WORKSPACE_CYCLES"), "{stderr}");
    assert!(stderr.contains(CYCLE_MESSAGE), "{stderr}");
}

/// `ignoreWorkspaceCycles` wins: nothing is reported at all, so the
/// install succeeds even though cycles are disallowed.
#[test]
fn ignore_workspace_cycles_wins_over_disallow_workspace_cycles() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_cyclic_workspace(
        &workspace,
        "ignoreWorkspaceCycles: true\ndisallowWorkspaceCycles: true\n",
    );

    let output = recursive_install(&workspace).assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(!stdout.contains(CYCLE_MESSAGE), "{stdout}");
}

#[test]
fn an_acyclic_workspace_is_not_reported() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    for (name, dependencies) in
        [("project-1", json!({ "project-2": "workspace:*" })), ("project-2", json!({}))]
    {
        let dir = workspace.join("packages").join(name);
        fs::create_dir_all(&dir).expect("create project dir");
        fs::write(
            dir.join("package.json"),
            json!({ "name": name, "version": "1.0.0", "dependencies": dependencies }).to_string(),
        )
        .expect("write package.json");
    }

    let output = recursive_install(&workspace).assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(!stdout.contains(CYCLE_MESSAGE), "{stdout}");
}

/// pnpm reaches this through its recursive-by-default dispatch;
/// pacquet's install spans the workspace without the flag.
#[test]
fn a_plain_workspace_install_warns_about_cyclic_workspace_dependencies() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_cyclic_workspace(&workspace, "");

    let output = install_command(&workspace, false).assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains(&format!("[WARN] {CYCLE_MESSAGE}")), "{stdout}");
}

#[test]
fn a_plain_workspace_install_fails_under_disallow_workspace_cycles() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_cyclic_workspace(&workspace, "disallowWorkspaceCycles: true\n");

    let output = install_command(&workspace, false).assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_DISALLOW_WORKSPACE_CYCLES"), "{stderr}");
}

/// Adding a dependency to one project is not the moment to talk about a
/// cycle between two others, and pnpm stays quiet there too.
#[test]
fn adding_a_dependency_to_one_project_reports_no_cycles() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_cyclic_workspace(&workspace, "");

    let mut add = Command::cargo_bin("pnpm").unwrap();
    add.current_dir(workspace.join("packages/project-1"));
    let output = add.args(["add", "is-odd@3.0.1"]).assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(!stdout.contains(CYCLE_MESSAGE), "{stdout}");
}

/// pnpm's `installDeps` returns from its optimistic repeat-install
/// short-circuit before it reaches the cycle check, so an install that
/// concludes "Already up to date" says nothing about cycles — not even
/// under `disallowWorkspaceCycles`, which cannot fail an install that
/// never ran.
#[test]
fn an_already_up_to_date_install_reports_no_cycles() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_cyclic_workspace(&workspace, "");

    let first = install_command(&workspace, false).assert().success();
    assert!(
        String::from_utf8_lossy(&first.get_output().stdout).contains(CYCLE_MESSAGE),
        "the install that did the work reports the cycle",
    );

    let output = install_command(&workspace, false).assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("Already up to date"), "{stdout}");
    assert!(!stdout.contains(CYCLE_MESSAGE), "{stdout}");

    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    yaml.push_str("disallowWorkspaceCycles: true\n");
    fs::write(&workspace_yaml, yaml).expect("write pnpm-workspace.yaml");

    let output = install_command(&workspace, false).assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("Already up to date"), "{stdout}");
}
