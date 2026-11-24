//! The checks footer line that names the direct dependencies whose
//! registry `latest` is newer than what was installed.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils_registry::bin::{
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

/// The summary line that adds [`FOO`], and the footer line naming newer
/// versions, if any.
fn added_line_and_note(command: Command, args: &[&str]) -> (String, Option<String>) {
    let assert = command
        .with_args(args)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let added = stdout
        .lines()
        .find(|line| line.starts_with(&format!("+ {FOO} ")))
        .unwrap_or_else(|| panic!("no summary line adds {FOO}:\n{stdout}"))
        .to_owned();
    let note = stdout
        .lines()
        .filter(|line| line.starts_with("· "))
        .find(|line| line.ends_with(r#"(see "pnpm outdated")"#))
        .map(ToOwned::to_owned);
    (added, note)
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
        added_line_and_note(pacquet, &["install"]),
        (
            format!("+ {FOO} 100.0.0"),
            Some(format!(r#"· 1 dependency has a newer version: {FOO} (see "pnpm outdated")"#)),
        ),
    );

    drop((root, mock_instance));
}

#[test]
fn install_of_the_latest_version_names_no_newer_version() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_manifest(&workspace, "^100.0.0");

    assert_eq!(added_line_and_note(pacquet, &["install"]), (format!("+ {FOO} 100.1.0"), None));

    drop((root, mock_instance));
}

/// A frozen install reads no registry metadata, so it has no `latest` to
/// name, as with pnpm v11's headless install.
#[test]
fn frozen_install_names_no_newer_version() {
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
        added_line_and_note(pacquet_at(&workspace), &["install", "--frozen-lockfile"]),
        (format!("+ {FOO} 100.0.0"), None),
    );

    drop((root, mock_instance));
}
