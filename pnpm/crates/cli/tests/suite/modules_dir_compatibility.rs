//! A modules directory linked with another store, virtual store, or layout
//! version: `add`, `remove`, and `update` refuse it, and `install`
//! recreates it. Mirrors `validateModules` and `checkCompatibility` in
//! `pnpm11/installing/deps-installer/src/install/`.

use crate::_utils::{
    append_workspace_yaml_key, enable_gvs_in_workspace_yaml, flatten_report, pacquet_in,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pipe_trait::Pipe;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[test]
fn add_remove_and_update_refuse_a_modules_dir_linked_from_another_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    install_is_positive(&workspace);

    switch_store_dir(&workspace);

    for args in [&["add", "is-negative@1.0.0"][..], &["remove", "is-positive"], &["update"]] {
        assert_report_contains(
            &run_failing(&workspace, args),
            &[
                "ERR_PNPM_UNEXPECTED_STORE",
                "Unexpected store location",
                "are currently linked from the store at",
                "pacquet-store",
                "pnpm now wants to use the store at",
                "other-store",
                "to link dependencies.",
            ],
        );
        assert!(
            workspace.join("node_modules/is-positive").exists(),
            "{args:?} purged node_modules",
        );
    }

    let other_store = Path::new("other-store").join("v11");
    install_records(&workspace, "storeDir", &other_store);
    let workspace_state = read_json(&workspace.join("node_modules/.pnpm-workspace-state-v1.json"));
    let recorded_store_dir = recorded_path(&workspace_state["settings"], "storeDir");
    assert!(
        recorded_store_dir.is_absolute() && recorded_store_dir.ends_with(&other_store),
        "{recorded_store_dir:?}",
    );

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

    install_records(&workspace, "virtualStoreDir", Path::new(".vs"));

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
        &[
            "ERR_PNPM_MODULES_BREAKING_CHANGE",
            "The current version of pnpm is not compatible with the available node_modules structure",
            "Run pnpm install to recreate node_modules.",
        ],
    );
    assert!(workspace.join("node_modules/is-positive").exists(), "add purged node_modules");

    drop((root, mock_instance));
}

/// `validateModules` purges only for a plain install: `add` keeps a modules
/// directory whose drift it does not report, such as a `nodeLinker` change.
#[test]
fn add_keeps_a_modules_dir_installed_with_another_node_linker() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    for name in ["dep", "dep2"] {
        let dir = root.path().join(name);
        fs::create_dir_all(&dir).expect("create the dependency");
        fs::write(
            dir.join("package.json"),
            serde_json::json!({ "name": name, "version": "1.0.0" }).to_string(),
        )
        .expect("write the dependency manifest");
    }
    pacquet_in(&workspace)
        .with_args(["add", "file:../dep"])
        .assert()
        .success();
    let marker = workspace.join("node_modules/marker.txt");
    fs::write(&marker, "kept").expect("write the marker");
    append_workspace_yaml_key(&workspace, "nodeLinker", "hoisted");

    pacquet_in(&workspace)
        .with_args(["add", "file:../dep2"])
        .assert()
        .success();

    assert!(marker.exists(), "add purged node_modules");

    drop((root, mock_instance));
}

/// A script runs the same from whichever store `node_modules` was linked
/// from, so `pnpm run` passes verification and keeps the recorded store for
/// the install that recreates the tree.
#[test]
fn run_passes_verification_after_a_store_change_that_install_acts_on() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "scripts": { "start": "node --version" },
            "dependencies": { "is-positive": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    switch_store_dir(&workspace);

    pacquet_in(&workspace)
        .with_args(["--config.verify-deps-before-run=error", "run", "start"])
        .assert()
        .success();

    let state = read_json(&workspace.join("node_modules/.pnpm-workspace-state-v1.json"));
    let recorded_store_dir = recorded_path(&state["settings"], "storeDir");
    assert!(
        recorded_store_dir.ends_with(Path::new("pacquet-store").join("v11")),
        "{recorded_store_dir:?}",
    );
    install_records(&workspace, "storeDir", &Path::new("other-store").join("v11"));

    drop((root, mock_instance));
}

/// Under a global virtual store, the state records the configured
/// `virtualStoreDir` anchored at the project, as pnpm v11 does, not the
/// global virtual store.
#[test]
fn the_workspace_state_records_the_configured_virtual_store_dir_under_a_global_virtual_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    enable_gvs_in_workspace_yaml(
        &workspace,
        "virtualStoreDir: node_modules/.vs\nglobalVirtualStoreDir: ../gvs\n",
    );
    install_is_positive(&workspace);

    let state = read_json(&workspace.join("node_modules/.pnpm-workspace-state-v1.json"));
    let recorded = recorded_path(&state["settings"], "virtualStoreDir");
    assert!(
        recorded.is_absolute()
            && recorded.ends_with(Path::new("workspace").join("node_modules").join(".vs")),
        "{recorded:?}",
    );

    drop((root, mock_instance));
}

fn switch_store_dir(workspace: &Path) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    let switched = yaml.replace("storeDir: ../pacquet-store", "storeDir: ../other-store");
    assert_ne!(yaml, switched, "the harness should write `storeDir: ../pacquet-store`");
    fs::write(&yaml_path, switched).expect("write pnpm-workspace.yaml");
}

fn install_is_positive(workspace: &Path) {
    pacquet_in(workspace)
        .with_args(["add", "is-positive@1.0.0"])
        .assert()
        .success();
}

/// `pnpm install` recreates the modules directory, whose `.modules.yaml`
/// then records a `field` path ending in `expected`.
fn install_records(workspace: &Path, field: &str, expected: &Path) {
    pacquet_in(workspace)
        .with_arg("install")
        .assert()
        .success();
    assert!(workspace.join("node_modules/is-positive").exists());
    let modules = read_json(&workspace.join("node_modules/.modules.yaml"));
    let recorded = recorded_path(&modules, field);
    assert!(recorded.ends_with(expected), "{field}: {recorded:?}");
}

fn read_json(path: &Path) -> serde_json::Value {
    fs::read_to_string(path)
        .expect("read the file")
        .pipe_as_ref(serde_json::from_str)
        .expect("parse the file as JSON")
}

fn recorded_path(record: &serde_json::Value, field: &str) -> PathBuf {
    record[field]
        .as_str()
        .map(|raw| pnpm_fs::lexical_normalize(Path::new(raw)))
        .unwrap_or_else(|| panic!("{field} is recorded"))
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
