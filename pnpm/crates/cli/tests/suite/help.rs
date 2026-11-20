use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use pretty_assertions::assert_eq;
use std::process::Command;

fn help_stdout(args: &[&str]) -> String {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(args)
        .output()
        .expect("run pnpm help");
    drop(root);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "pnpm {args:?} failed: {stderr}");
    String::from_utf8(output.stdout).expect("help output is UTF-8")
}

#[test]
fn help_all_lists_every_command() {
    let help = help_stdout(&["help"]);
    for command in ["cat-index", "find-hash", "store", "cache"] {
        assert!(help.contains(&format!("\n  {command} ")), "{command} missing from: {help}");
    }
    assert_eq!(help_stdout(&["help", "--all"]), help);
    assert_eq!(help_stdout(&["help", "-a"]), help);
    assert_eq!(help_stdout(&["help", "--all=true"]), help);
    assert_eq!(help_stdout(&["help", "--no-all"]), help);
}

#[test]
fn help_all_with_a_command_prints_that_command() {
    let install_help = help_stdout(&["help", "install"]);
    assert_eq!(help_stdout(&["help", "-a", "install"]), install_help);
    assert_eq!(help_stdout(&["help", "install", "--all"]), install_help);
}
