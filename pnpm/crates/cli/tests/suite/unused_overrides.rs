//! The warning about `overrides` entries that matched no dependency.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{
    AddMockedRegistry,
    CommandTempCwd,
};
use std::{
    fs,
    path::Path,
    process::Command,
};

const PROD_DEP: &str = "@pnpm.e2e/foo";
const DEV_DEP: &str = "@pnpm.e2e/bar";
const MISSING: &str = "@pnpm.e2e/missing-pkg";

/// Write a manifest with one production and one development dependency, and
/// append `settings` to the `pnpm-workspace.yaml` the mocked registry wrote.
fn write_fixture(workspace: &Path, settings: &str) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "unused-overrides-fixture",
            "version": "1.0.0",
            "dependencies": { PROD_DEP: "100.0.0" },
            "devDependencies": { DEV_DEP: "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    append_workspace_yaml(workspace, settings);
}

fn append_workspace_yaml(workspace: &Path, settings: &str) {
    let path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&path).unwrap_or_default();
    if !yaml.is_empty() && !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str(settings);
    fs::write(&path, yaml).expect("update pnpm-workspace.yaml");
}

fn overrides_block() -> String {
    format!(
        "overrides:\n  \"{PROD_DEP}\": 100.0.0\n  \"{DEV_DEP}\": 100.0.0\n  \"{MISSING}\": 1.0.0\n"
    )
}

#[test]
fn install_reports_only_the_override_that_matched_no_dependency() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_fixture(&workspace, &overrides_block());

    let output = pacquet
        .with_arg("install")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    eprintln!("STDOUT:\n{stdout}\n");

    assert!(stdout.contains(&format!("1 override matched no dependency: {MISSING}")));

    drop((root, mock_instance));
}

/// A legacy deploy resolves only the deployed project's production
/// dependencies, so it cannot tell whether an override matches anything else.
#[test]
fn legacy_deploy_reports_no_unused_override() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_fixture(&workspace, &overrides_block());
    append_workspace_yaml(
        &workspace,
        "lockfile: false\npackages:\n  - 'packages/*'\ninjectWorkspacePackages: true\n",
    );
    let app = workspace.join("packages/app");
    fs::create_dir_all(&app).expect("create packages/app");
    fs::write(
        app.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { PROD_DEP: "100.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/app/package.json");

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["--filter", "app", "deploy", "--legacy", "--prod", "deploy"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    eprintln!("STDOUT:\n{stdout}\n");

    assert!(
        workspace
            .join("deploy/node_modules")
            .join(PROD_DEP)
            .exists(),
        "the deployed project's dependency must be installed",
    );
    assert!(!stdout.contains("matched no dependency"));

    drop((root, mock_instance));
}
