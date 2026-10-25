//! What the pnpmfile's `preResolution` hook changes in the lockfiles takes
//! effect, as in pnpm, where the hook receives the install's own lockfile
//! objects.

use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::bump_mtime,
};
use std::{fs, path::Path};

/// Empties the wanted lockfile once a `clear-lockfile` file exists next to
/// the pnpmfile, so the pnpmfile and its checksum stay the same across runs.
const CLEARING_PNPMFILE: &str = r"const fs = require('fs');
const path = require('path');
module.exports = { hooks: { preResolution (ctx) {
  if (!fs.existsSync(path.join(__dirname, 'clear-lockfile'))) return;
  ctx.wantedLockfile.importers = {};
  delete ctx.wantedLockfile.packages;
  delete ctx.wantedLockfile.snapshots;
} } };
";

fn write_manifest(workspace: &Path, foo_spec: &str) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@pnpm.e2e/foo": foo_spec } }).to_string(),
    )
    .expect("write package.json");
}

fn installed_foo_version(workspace: &Path) -> String {
    let manifest = workspace.join("node_modules/@pnpm.e2e/foo/package.json");
    let manifest = fs::read_to_string(&manifest).expect("read the installed manifest");
    let manifest: serde_json::Value = serde_json::from_str(&manifest).expect("parse manifest");
    manifest["version"]
        .as_str()
        .expect("version")
        .to_string()
}

/// Installs `@pnpm.e2e/foo@100.0.0` with [`CLEARING_PNPMFILE`], then turns
/// the hook on. The registry's latest `@pnpm.e2e/foo` is 100.1.0.
fn install_pinned_foo(workspace: &Path) {
    fs::write(workspace.join(".pnpmfile.cjs"), CLEARING_PNPMFILE).expect("write pnpmfile");
    write_manifest(workspace, "100.0.0");
    pacquet_in(workspace)
        .with_arg("install")
        .assert()
        .success();
    fs::write(workspace.join("clear-lockfile"), "").expect("write marker");
}

/// Without the hook, the install keeps the locked 100.0.0, which satisfies
/// the widened range.
#[test]
fn resolution_starts_from_the_lockfile_the_hook_changed() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    install_pinned_foo(&workspace);
    write_manifest(&workspace, "^100.0.0");
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert_eq!(installed_foo_version(&workspace), "100.1.0");
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(lockfile.contains("'@pnpm.e2e/foo@100.1.0':"), "LOCKFILE:\n{lockfile}");

    drop((root, mock_instance));
}

#[test]
fn a_frozen_install_checks_the_lockfile_the_hook_changed() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    install_pinned_foo(&workspace);
    bump_mtime(&workspace.join("package.json"));
    let output = pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .output()
        .expect("run install");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "STDERR:\n{stderr}");
    assert!(stderr.contains("ERR_PNPM_PACKAGE_MANAGER_NO_IMPORTER"), "STDERR:\n{stderr}");

    drop((root, mock_instance));
}

#[test]
fn each_pnpmfile_sees_the_lockfile_the_previous_one_changed() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_manifest(&workspace, "100.0.0");
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    fs::write(
        workspace.join("first.cjs"),
        "module.exports = { hooks: { preResolution (ctx) { ctx.wantedLockfile.importers = {} } } }",
    )
    .expect("write first pnpmfile");
    fs::write(
        workspace.join("second.cjs"),
        r"module.exports = { hooks: { preResolution (ctx) {
  const count = Object.keys(ctx.wantedLockfile.importers).length;
  require('fs').writeFileSync(require('path').join(__dirname, 'importers-seen'), String(count));
} } };",
    )
    .expect("write second pnpmfile");
    fs::write(workspace.join("pnpm-workspace.yaml"), "pnpmfile:\n  - first.cjs\n  - second.cjs\n")
        .expect("write pnpm-workspace.yaml");

    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let seen = fs::read_to_string(workspace.join("importers-seen")).expect("read importers-seen");
    assert_eq!(seen, "0");

    drop((root, mock_instance));
}

#[test]
fn a_hook_that_breaks_the_lockfile_fails_the_install() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_manifest(&workspace, "100.0.0");
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { preResolution (ctx) { ctx.wantedLockfile.importers = 42 } } }",
    )
    .expect("write pnpmfile");
    let output = pacquet_in(&workspace)
        .with_arg("install")
        .output()
        .expect("run install");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "STDERR:\n{stderr}");
    assert!(stderr.contains("ERR_PNPM_BAD_PRE_RESOLUTION_HOOK_RESULT"), "STDERR:\n{stderr}");
    assert!(stderr.contains("wantedLockfile"), "STDERR:\n{stderr}");

    drop((root, mock_instance));
}

#[test]
fn a_throwing_hook_fails_the_install() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { preResolution () { throw new Error('lockfile rejected') } } }",
    )
    .expect("write pnpmfile");

    let output = pacquet_in(&workspace)
        .with_arg("install")
        .output()
        .expect("run install");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "STDERR:\n{stderr}");
    assert!(stderr.contains("ERR_PNPM_PNPMFILE_FAIL"), "STDERR:\n{stderr}");
    assert!(stderr.contains("lockfile rejected"), "STDERR:\n{stderr}");

    drop(root);
}
