//! A `--global` command rejects the command-line settings it cannot
//! honour, with pnpm's `CONFIG_CONFLICT_*_WITH_GLOBAL` errors.

use super::{CommandTempCwd, Path, global_command};
use assert_cmd::assert::OutputAssertExt;
use command_extra::CommandExtra;
use pnpm_testing_utils::diagnostics::assert_diagnostic_contains;

fn assert_conflict(args: &[&str], code: &str, setting: &str) {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm-home");

    let output = global_command(&workspace, &pnpm_home)
        .with_args(args)
        .output()
        .expect("spawn pnpm");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{args:?} must fail:\n{stderr}");
    assert_diagnostic_contains(&stderr, code);
    assert_diagnostic_contains(
        &stderr,
        &format!(r#"Configuration conflict. "{setting}" may not be used with "global""#),
    );
}

#[test]
fn virtual_store_dir_conflicts_with_global() {
    for args in [
        ["add", "--global", "is-positive", "--virtual-store-dir", "vstore"].as_slice(),
        &["add", "--global", "is-positive", "--config.virtual-store-dir=vstore"],
        &["remove", "--global", "is-positive", "--virtual-store-dir=vstore"],
        &["list", "--global", "--virtual-store-dir=vstore"],
    ] {
        assert_conflict(
            args,
            "ERR_PNPM_CONFIG_CONFLICT_VIRTUAL_STORE_DIR_WITH_GLOBAL",
            "virtual-store-dir",
        );
    }
}

#[test]
fn custom_hoist_pattern_conflicts_with_global() {
    for args in [
        ["add", "--global", "is-positive", "--hoist-pattern", "eslint"].as_slice(),
        &["update", "--global", "--hoist-pattern=*", "--hoist-pattern=eslint"],
    ] {
        assert_conflict(
            args,
            "ERR_PNPM_CONFIG_CONFLICT_HOIST_PATTERN_WITH_GLOBAL",
            "hoist-pattern",
        );
    }
}

#[test]
fn workspace_settings_conflict_with_global() {
    assert_conflict(
        &["add", "--global", "is-positive", "--link-workspace-packages"],
        "ERR_PNPM_CONFIG_CONFLICT_LINK_WORKSPACE_PACKAGES_WITH_GLOBAL",
        "link-workspace-packages",
    );
    assert_conflict(
        &["add", "--global", "is-positive", "--shared-workspace-lockfile"],
        "ERR_PNPM_CONFIG_CONFLICT_SHARED_WORKSPACE_LOCKFILE_WITH_GLOBAL",
        "shared-workspace-lockfile",
    );
    assert_conflict(
        &["add", "--global", "is-positive", "--config.lockfile-dir=."],
        "ERR_PNPM_CONFIG_CONFLICT_LOCKFILE_DIR_WITH_GLOBAL",
        "lockfile-dir",
    );
}

/// Only a value given on the command line conflicts. The default hoist
/// pattern, a disabled workspace setting, and a setting from the
/// environment are accepted.
#[test]
fn accepted_settings_pass_with_global() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm-home");
    std::fs::create_dir_all(pnpm_home.join("bin")).expect("create global bin dir");
    let list = |extra: &[&str], pnpm_home: &Path| {
        global_command(&workspace, pnpm_home)
            .with_args(["list", "--global"])
            .with_args(extra)
    };

    list(&["--hoist-pattern=*"], &pnpm_home).assert().success();
    list(&["--no-link-workspace-packages", "--no-shared-workspace-lockfile"], &pnpm_home)
        .assert()
        .success();
    list(&[], &pnpm_home)
        .with_env("PNPM_CONFIG_VIRTUAL_STORE_DIR", "vstore")
        .with_env("PNPM_CONFIG_HOIST_PATTERN", "eslint")
        .assert()
        .success();
}
