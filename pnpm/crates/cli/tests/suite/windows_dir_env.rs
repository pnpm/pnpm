#![cfg(windows)]

use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, diagnostics::assert_diagnostic_contains};
use std::fs;

const UNRESOLVED_PNPM_HOME: &str = r"%PNPM_TEST_UNSET_VARIABLE%\pnpm";

fn assert_rejects_unresolved_pnpm_home(args: &[&str]) {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");

    let output = pacquet
        .with_env("PNPM_HOME", UNRESOLVED_PNPM_HOME)
        .with_args(args)
        .output()
        .expect("run pnpm");

    assert!(!output.status.success(), "pnpm {args:?} should fail: {output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_diagnostic_contains(&stderr, "ERR_PNPM_UNEXPANDED_ENV_IN_PATH");
    assert_diagnostic_contains(
        &stderr,
        "PNPM_HOME contains an unexpanded environment variable: %PNPM_TEST_UNSET_VARIABLE%",
    );
    assert!(
        !workspace.join("%PNPM_TEST_UNSET_VARIABLE%").exists(),
        "pnpm {args:?} created a directory named after the unresolved reference",
    );
    drop(root);
}

#[test]
fn install_rejects_an_unresolved_reference_in_pnpm_home() {
    assert_rejects_unresolved_pnpm_home(&["install"]);
}

#[test]
fn setup_rejects_an_unresolved_reference_in_pnpm_home() {
    assert_rejects_unresolved_pnpm_home(&["setup"]);
}
