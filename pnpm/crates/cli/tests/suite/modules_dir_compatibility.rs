//! A modules directory linked with another store, virtual store, or layout
//! version: `add`, `remove`, and `update` refuse it, and `install`
//! recreates it. Mirrors `validateModules` and `checkCompatibility` in
//! `pnpm11/installing/deps-installer/src/install/`.

use crate::_utils::{append_workspace_yaml_key, flatten_report, pacquet_in};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

#[test]
fn add_remove_and_update_refuse_a_modules_dir_linked_from_another_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    install_is_positive(&workspace);

    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    let switched = yaml.replace("storeDir: ../pacquet-store", "storeDir: ../other-store");
    assert_ne!(yaml, switched, "the harness should write `storeDir: ../pacquet-store`");
    fs::write(&yaml_path, switched).expect("write pnpm-workspace.yaml");

    for args in [&["add", "is-negative@1.0.0"][..], &["remove", "is-positive"], &["update"]] {
        assert_report_contains(
            &run_failing(&workspace, args),
            &[
                "ERR_PNPM_UNEXPECTED_STORE",
                "Unexpected store location",
                r#"are currently linked from the store at ""#,
                r#"/pacquet-store/v11". pnpm now wants to use the store at ""#,
                r#"/other-store/v11" to link dependencies."#,
            ],
        );
        assert!(
            workspace.join("node_modules/is-positive").exists(),
            "{args:?} purged node_modules",
        );
    }

    // The workspace state does not record `storeDir`, so the optimistic
    // repeat install would report the project up to date.
    pacquet_in(&workspace)
        .with_args(["install", "--config.optimistic-repeat-install=false"])
        .assert()
        .success();
    assert!(workspace.join("node_modules/is-positive").exists());
    let modules_yaml = fs::read_to_string(workspace.join("node_modules/.modules.yaml"))
        .expect("read .modules.yaml");
    assert!(modules_yaml.contains(r#"/other-store/v11""#), "{modules_yaml}");

    drop((root, mock_instance));
}

#[test]
fn add_refuses_a_modules_dir_linked_from_another_virtual_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    install_is_positive(&workspace);
    append_workspace_yaml_key(&workspace, "virtualStoreDir", "node_modules/.vs");

    assert_report_contains(
        &run_failing(&workspace, &["add", "is-negative@1.0.0"]),
        &[
            "ERR_PNPM_UNEXPECTED_VIRTUAL_STORE",
            "Unexpected virtual store location",
            "pnpm now wants to use the virtual store at",
        ],
    );
    assert!(workspace.join("node_modules/is-positive").exists(), "add purged node_modules");

    drop((root, mock_instance));
}

#[test]
fn add_refuses_a_modules_dir_with_another_layout_version() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    install_is_positive(&workspace);

    let modules_yaml_path = workspace.join("node_modules/.modules.yaml");
    let modules_yaml = fs::read_to_string(&modules_yaml_path).expect("read .modules.yaml");
    let downgraded = modules_yaml.replace(r#""layoutVersion": 5"#, r#""layoutVersion": 4"#);
    assert_ne!(modules_yaml, downgraded, "`.modules.yaml` should record layout version 5");
    fs::write(&modules_yaml_path, downgraded).expect("write .modules.yaml");

    assert_report_contains(
        &run_failing(&workspace, &["add", "is-negative@1.0.0"]),
        &["ERR_PNPM_MODULES_BREAKING_CHANGE"],
    );
    assert!(workspace.join("node_modules/is-positive").exists(), "add purged node_modules");

    drop((root, mock_instance));
}

fn install_is_positive(workspace: &Path) {
    pacquet_in(workspace)
        .with_args(["add", "is-positive@1.0.0"])
        .assert()
        .success();
}

fn run_failing(workspace: &Path, args: &[&str]) -> String {
    let output = pacquet_in(workspace)
        .with_args(args)
        .assert()
        .failure();
    String::from_utf8_lossy(&output.get_output().stderr).into_owned()
}

fn assert_report_contains(stderr: &str, expected: &[&str]) {
    let report = flatten_report(stderr);
    for text in expected {
        assert!(report.contains(&flatten_report(text)), "expected {text:?} in {stderr}");
    }
}
