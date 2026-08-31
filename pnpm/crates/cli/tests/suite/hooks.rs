use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::fs;

#[test]
fn filter_log_is_ignored_with_a_warning() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { filterLog: () => false } }",
    )
    .expect("write filterLog hook");
    fs::write(workspace.join("pnpm-lock.yaml"), "not: [valid").expect("write broken lockfile");

    let output = pacquet_in(&workspace).with_arg("install").assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("filterLog hook is deprecated"), "STDOUT:\n{stdout}");
    assert!(stdout.contains("Ignoring broken lockfile"), "STDOUT:\n{stdout}");

    drop(root);
}
