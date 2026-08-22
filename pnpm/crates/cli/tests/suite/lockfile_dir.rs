//! The `lockfileDir` setting and its `--lockfile-dir` flag.
//!
//! Pinning the lockfile directory moves the whole shared layout with it:
//! `pnpm-lock.yaml`, the root `node_modules` holding the virtual store,
//! and the importer ids, which become the paths from the pin down to each
//! project. Every project keeps its own `node_modules` of symlinks.

use crate::_utils::append_workspace_yaml_key;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

/// The `importers:` keys of the lockfile at `lockfile_dir`.
fn importer_ids(lockfile_dir: &Path) -> Vec<String> {
    pnpm_lockfile::Lockfile::load_wanted_from_dir(lockfile_dir)
        .expect("load pnpm-lock.yaml")
        .expect("pnpm-lock.yaml exists at the pinned lockfile dir")
        .importers
        .into_keys()
        .collect()
}

/// `--lockfile-dir` puts `pnpm-lock.yaml` and the virtual store in a
/// directory above the project, and names the project by its path from
/// there. The project still gets its own `node_modules` with the
/// dependency linked in. Ports pnpm's "install with external lockfile
/// directory".
#[test]
fn external_lockfile_dir_holds_the_lockfile_and_the_virtual_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let lockfile_dir = workspace.join("nested");
    let project_dir = lockfile_dir.join("project");
    fs::create_dir_all(&project_dir).expect("create the project dir");
    fs::write(project_dir.join("package.json"), r#"{"name":"project","version":"1.0.0"}"#)
        .expect("write package.json");

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&project_dir)
        .with_args(["install", "is-positive@1.0.0", "--lockfile-dir", ".."])
        .assert()
        .success();

    assert_eq!(importer_ids(&lockfile_dir), ["project"]);
    assert!(
        !workspace.join("pnpm-lock.yaml").exists(),
        "no lockfile may be written at the workspace root the pin moved away from",
    );
    assert!(
        lockfile_dir.join("node_modules/.pnpm/is-positive@1.0.0").is_dir(),
        "the virtual store must live under the pinned lockfile dir",
    );
    assert!(
        project_dir.join("node_modules/is-positive/package.json").is_file(),
        "the project keeps its own node_modules of symlinks into the virtual store",
    );

    drop((root, mock_instance));
}

/// The `lockfileDir` setting is read from `pnpm-workspace.yaml` and
/// resolved against it, so a workspace can keep its lockfile one level up
/// without passing a flag on every command.
#[test]
fn lockfile_dir_setting_is_read_from_the_workspace_manifest() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("package.json"), r#"{"name":"project","version":"1.0.0"}"#)
        .expect("write package.json");
    append_workspace_yaml_key(&workspace, "lockfileDir", "..");

    pacquet.with_args(["install", "is-positive@1.0.0"]).assert().success();

    let lockfile_dir = root.path();
    assert_eq!(importer_ids(lockfile_dir), ["workspace"]);
    assert!(
        !workspace.join("pnpm-lock.yaml").exists(),
        "no lockfile may be written at the workspace root the setting moved away from",
    );
    assert!(
        lockfile_dir.join("node_modules/.pnpm/is-positive@1.0.0").is_dir(),
        "the virtual store must live under the configured lockfile dir",
    );
    assert!(
        workspace.join("node_modules/is-positive/package.json").is_file(),
        "the project keeps its own node_modules of symlinks into the virtual store",
    );

    drop(mock_instance);
}

/// A global install owns the lockfile in its own group directory, so
/// pnpm refuses to let `--lockfile-dir` redirect it.
#[test]
fn lockfile_dir_conflicts_with_global() {
    let CommandTempCwd { pacquet, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let output = pacquet
        .with_args(["add", "--global", "is-positive@1.0.0", "--lockfile-dir", "."])
        .output()
        .expect("spawn pacquet add");

    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(!output.status.success(), "the conflicting flags must fail the command:\n{stderr}");
    assert!(
        stderr.contains("ERR_PNPM_CONFIG_CONFLICT_LOCKFILE_DIR_WITH_GLOBAL"),
        "the error must carry pnpm's code:\n{stderr}",
    );

    drop((root, mock_instance));
}
