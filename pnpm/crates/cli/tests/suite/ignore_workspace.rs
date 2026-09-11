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

/// A project nested under a workspace root but absent from its `packages`
/// patterns is standalone under `--ignore-workspace`. The install-family
/// commands must not re-discover the workspace through the ancestor walk:
/// doing so anchors the lockfile and the importer ids on the workspace
/// root and pulls in every sibling project.
fn assert_only_the_nested_project_is_installed(subcommands: &[&str]) {
    assert_only_the_nested_project_is_installed_with(subcommands, None);
}

/// `expected_last_output` asserts a marker in the final command's output,
/// which is how a caller pins *which* install path ran rather than only the
/// filesystem state it left behind.
fn assert_only_the_nested_project_is_installed_with(
    subcommands: &[&str],
    expected_last_output: Option<&str>,
) {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["packages/*"], &["packages/alfa"]);
    let nested = workspace.join("nested");
    fs::create_dir_all(&nested).expect("create the nested project dir");
    fs::write(
        nested.join("package.json"),
        json!({ "name": "nested", "version": "1.0.0" }).to_string(),
    )
    .expect("write the nested package.json");

    let mut printed = String::new();
    for subcommand in subcommands {
        let output = pacquet_in(&nested)
            .with_args([subcommand, "--ignore-workspace"])
            .output()
            .expect("spawn pacquet");
        assert!(output.status.success(), "{subcommand} failed: {output:?}");
        printed = String::from_utf8_lossy(&output.stdout).into_owned();
    }
    if let Some(expected) = expected_last_output {
        assert!(printed.contains(expected), "expected {expected:?} in:\n{printed}");
    }

    assert!(nested.join("node_modules").is_dir(), "the nested project is the one installed");
    assert!(nested.join("pnpm-lock.yaml").is_file(), "the lockfile belongs to the nested project");
    assert!(
        !workspace.join("pnpm-lock.yaml").exists(),
        "the lockfile must not be anchored on the ignored workspace root",
    );
    assert!(
        !workspace.join("packages/alfa/node_modules").exists(),
        "a sibling project of the ignored workspace must not be installed",
    );
    assert!(
        !nested.join("packages").exists(),
        "workspace importers must not be re-rooted at the current directory",
    );

    drop(root);
}

#[test]
fn ignore_workspace_installs_only_the_nested_project() {
    assert_only_the_nested_project_is_installed(&["install"]);
}

#[test]
fn ignore_workspace_updates_only_the_nested_project() {
    assert_only_the_nested_project_is_installed(&["update"]);
}

/// A second `install` over an unchanged project takes the repeat-install
/// fast path, which loads a configuration of its own. It has to be seeded
/// with the flag as well, or it answers "is this up to date?" for the
/// workspace above the ignored project.
#[test]
fn ignore_workspace_survives_the_repeat_install_fast_path() {
    // "Already up to date" is the fast path's own marker: without it the
    // second install fell through to a full one, which would leave the same
    // files behind and hide a regression here.
    assert_only_the_nested_project_is_installed_with(
        &["install", "install"],
        Some("Already up to date"),
    );
}

/// The install counterpart of
/// [`a_configured_ignore_workspace_does_not_suppress_the_search`]: a value
/// arriving from the environment lands after the workspace search, so it
/// leaves the discovered workspace in place rather than making the project
/// standalone.
#[test]
fn a_configured_ignore_workspace_still_installs_the_workspace() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["packages/*"], &["packages/alfa"]);

    let output = pacquet_in(&workspace)
        .with_env("PNPM_CONFIG_IGNORE_WORKSPACE", "true")
        .with_args(["install"])
        .output()
        .expect("spawn pacquet");
    assert!(output.status.success(), "install failed: {output:?}");

    // The lockfile exists either way — a standalone install at the
    // workspace root writes one too. What separates the two is whether the
    // sibling project is an importer of it.
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read the lockfile");
    assert!(
        lockfile.contains("packages/alfa"),
        "the workspace projects are still importers: {lockfile}",
    );

    drop(root);
}

/// A directory below the ignored project is not a workspace project of it:
/// nothing declares it as one. Recursive-by-default promotion must not
/// consult the ancestor workspace either, or the selection discovers the
/// subdirectory and installs it as an importer of its own lockfile.
#[test]
fn ignore_workspace_does_not_install_subdirectories_of_the_nested_project() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["packages/*"], &["packages/alfa"]);
    let nested = workspace.join("nested");
    let child = nested.join("child");
    fs::create_dir_all(&child).expect("create the child project dir");
    fs::write(
        nested.join("package.json"),
        json!({ "name": "nested", "version": "1.0.0" }).to_string(),
    )
    .expect("write the nested package.json");
    fs::write(
        child.join("package.json"),
        json!({ "name": "child", "version": "1.0.0" }).to_string(),
    )
    .expect("write the child package.json");

    let output = pacquet_in(&nested)
        .with_args(["install", "--ignore-workspace"])
        .output()
        .expect("spawn pacquet");
    assert!(output.status.success(), "install failed: {output:?}");

    let lockfile = fs::read_to_string(nested.join("pnpm-lock.yaml")).expect("read the lockfile");
    assert!(!lockfile.contains("child"), "the subdirectory is not an importer: {lockfile}");
    assert!(!child.join("node_modules").exists(), "the subdirectory must not be installed");

    drop(root);
}
