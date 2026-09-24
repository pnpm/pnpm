use assert_cmd::prelude::*;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::{fs, path::Path, process::Command};

const NESTED_MANIFEST_WARNING: &str = "[WARN] The settings in services/inner/pnpm-workspace.yaml do not apply, because services/inner is a project of this workspace. pnpm reads settings only from the pnpm-workspace.yaml at the workspace root. Move the settings there, or add \"!services/inner\" to the root's \"packages\" to keep that project a separate workspace.";

fn write_workspace_with_nested_manifest(workspace: &Path, extra_settings: &str) {
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("packages:\n  - services/*\n{extra_settings}"),
    )
    .expect("write workspace manifest");
    fs::write(workspace.join("package.json"), json!({ "name": "outer" }).to_string())
        .expect("write root package.json");
    for name in ["inner", "plain"] {
        let dir = workspace.join("services").join(name);
        fs::create_dir_all(&dir).expect("create project dir");
        fs::write(dir.join("package.json"), json!({ "name": name }).to_string())
            .expect("write package.json");
    }
    fs::write(
        workspace.join("services/inner/pnpm-workspace.yaml"),
        "packages:\n  - \"!**\"\noverrides:\n  is-odd: 3.0.1\n",
    )
    .expect("write nested workspace manifest");
}

fn install(workspace: &Path, args: &[&str]) -> String {
    let output = Command::cargo_bin("pnpm")
        .unwrap()
        .current_dir(workspace)
        .args(args)
        .arg("install")
        .assert()
        .success();
    String::from_utf8_lossy(&output.get_output().stdout).into_owned()
}

#[test]
fn a_workspace_install_warns_that_a_nested_workspace_manifest_does_not_apply() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_workspace_with_nested_manifest(&workspace, "");

    let stdout = install(&workspace, &[]);
    assert_eq!(stdout.matches(NESTED_MANIFEST_WARNING).count(), 1, "{stdout}");
}

/// Each project installs on its own there, and the warning still comes once.
#[test]
fn a_workspace_with_a_lockfile_per_project_warns_once() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_workspace_with_nested_manifest(&workspace, "sharedWorkspaceLockfile: false\n");

    let stdout = install(&workspace, &[]);
    assert_eq!(stdout.matches(NESTED_MANIFEST_WARNING).count(), 1, "{stdout}");
}

#[test]
fn an_install_that_leaves_out_the_nested_workspace_does_not_warn() {
    let CommandTempCwd { root: _root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_workspace_with_nested_manifest(&workspace, "");

    let stdout = install(&workspace, &["--filter", "plain"]);
    assert!(!stdout.contains("pnpm-workspace.yaml do not apply"), "{stdout}");
}
