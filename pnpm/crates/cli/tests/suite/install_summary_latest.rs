//! The "(X is available)" note the install summary puts after a direct
//! dependency whose registry `latest` is newer than what was installed.

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

const FOO: &str = "@pnpm.e2e/foo";

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn write_manifest(workspace: &Path, range: &str) {
    let manifest = serde_json::json!({ "dependencies": { FOO: range } });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
}

/// The summary line that adds [`FOO`].
fn added_line(command: Command, args: &[&str]) -> String {
    let assert = command
        .with_args(args)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    stdout
        .lines()
        .find(|line| line.starts_with(&format!("+ {FOO} ")))
        .unwrap_or_else(|| panic!("no summary line adds {FOO}:\n{stdout}"))
        .to_owned()
}

#[test]
fn install_names_a_newer_latest_of_a_direct_dependency() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_manifest(&workspace, "100.0.0");

    assert_eq!(
        added_line(pacquet, &["install"]),
        format!("+ {FOO} 100.0.0 (100.1.0 is available)"),
    );

    drop((root, mock_instance));
}

#[test]
fn install_of_the_latest_version_prints_no_note() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_manifest(&workspace, "^100.0.0");

    assert_eq!(added_line(pacquet, &["install"]), format!("+ {FOO} 100.1.0"));

    drop((root, mock_instance));
}

/// A frozen install reads no registry metadata, so it has no `latest` to
/// name, as with pnpm v11's headless install.
#[test]
fn frozen_install_prints_no_note() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_manifest(&workspace, "100.0.0");
    pacquet
        .with_arg("install")
        .assert()
        .success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    assert_eq!(
        added_line(pacquet_at(&workspace), &["install", "--frozen-lockfile"]),
        format!("+ {FOO} 100.0.0"),
    );

    drop((root, mock_instance));
}
