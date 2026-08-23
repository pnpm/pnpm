//! `--ignore-workspace` and `--workspace-packages`: the two flags that
//! change which workspace, if any, a command belongs to. The scripts run
//! through pacquet's `sh -c` executor, so the file is gated to Unix.
#![cfg(unix)]

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, command_env::CommandTestExt};
use serde_json::json;
use std::{fs, path::Path, process::Command};

fn write_workspace(workspace: &Path, packages: &[&str], names: &[&str]) {
    let patterns = packages.iter().map(|name| format!("  - {name}")).collect::<Vec<_>>();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("packages:\n{}\nnodeLinker: hoisted\n", patterns.join("\n")),
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "workspace-root", "version": "1.0.0", "private": true }).to_string(),
    )
    .expect("write root package.json");
    for name in names {
        let dir = workspace.join(name);
        fs::create_dir_all(&dir).expect("create project dir");
        fs::write(
            dir.join("package.json"),
            json!({ "name": name, "version": "1.0.0" }).to_string(),
        )
        .expect("write package.json");
    }
}

/// A second `pnpm` command in the same workspace, for the tests that
/// compare two invocations.
fn pacquet_in(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
}

fn stdout_of(mut command: Command) -> String {
    let output = command.output().expect("spawn pacquet");
    assert!(output.status.success(), "command failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    eprintln!("STDOUT:\n{stdout}\n");
    stdout
}

/// `--ignore-workspace` stops the workspace search, so the settings the
/// workspace manifest carries never reach the configuration.
#[test]
fn ignore_workspace_drops_the_workspace_manifest_settings() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["packages/*"], &["packages/alfa"]);

    assert_eq!(
        stdout_of(pacquet.with_args(["config", "get", "nodeLinker"])),
        "hoisted",
        "the workspace manifest's setting applies by default",
    );
    assert_eq!(
        stdout_of(pacquet_in(&workspace).with_args([
            "--ignore-workspace",
            "config",
            "get",
            "nodeLinker"
        ])),
        "undefined",
    );

    drop(root);
}

/// pnpm resolves the workspace dir from argv alone, so the setting only
/// suppresses the search when it arrives as the flag. A configured value
/// still reaches the readers that treat it as a plain setting.
#[test]
fn a_configured_ignore_workspace_does_not_suppress_the_search() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["packages/*"], &["packages/alfa"]);

    assert_eq!(
        stdout_of(pacquet.with_env("PNPM_CONFIG_IGNORE_WORKSPACE", "true").with_args([
            "config",
            "get",
            "nodeLinker"
        ]),),
        "hoisted",
    );

    drop(root);
}

/// `--workspace-packages` replaces the manifest's `packages` patterns,
/// so the recursive selection follows the flag rather than the file.
#[test]
fn workspace_packages_overrides_the_manifest_patterns() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["packages/*"], &["packages/alfa", "packages/beta"]);

    let stdout = stdout_of(pacquet.with_args([
        "--workspace-packages",
        "packages/alfa",
        "--config.verify-deps-before-run=false",
        "-r",
        "exec",
        "pwd",
    ]));
    let selected = stdout.lines().collect::<Vec<_>>();
    assert_eq!(selected.len(), 1, "only alfa should be selected: {stdout}");
    assert!(selected[0].ends_with("packages/alfa"), "wrong project selected: {stdout}");

    drop(root);
}
