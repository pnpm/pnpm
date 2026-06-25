//! End-to-end tests for global package management (`add -g`, `remove -g`,
//! `update -g`, `list -g`). The happy paths need the mocked registry and
//! create real symlinks / bin shims, so they are Unix-gated.

use assert_cmd::cargo::CommandCargoExt;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Build a fresh `pacquet` command in `workspace` with `PNPM_HOME` set and
/// the global bin directory prepended to `PATH` (so `checkGlobalBinDir`
/// passes for the mutating commands).
#[cfg(unix)]
fn global_command(workspace: &Path, pnpm_home: &Path) -> Command {
    let global_bin = pnpm_home.join("bin");
    let existing_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{existing_path}", global_bin.display());
    Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(workspace)
        .with_env("PNPM_HOME", pnpm_home)
        .with_env("PATH", path)
}

#[cfg(unix)]
fn symlink_entries(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|ft| ft.is_symlink()))
        .map(|entry| entry.path())
        .collect()
}

/// `pacquet add -g <pkg>` installs the package under the global packages
/// directory, links its bin into the global bin directory, and records a
/// cache-keyed hash symlink. `list -g` then reports it, and `remove -g`
/// tears it all down.
#[cfg(unix)]
#[test]
fn global_add_list_remove_round_trip() {
    use assert_cmd::prelude::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    let global_pkg_dir = pnpm_home.join("global").join("v11");
    fs::create_dir_all(&global_bin).expect("create global bin dir");

    // add -g
    global_command(&workspace, &pnpm_home)
        .with_arg("add")
        .with_arg("-g")
        .with_arg("@foo/touch-file-one-bin")
        .assert()
        .success();

    assert!(
        global_bin.join("touch-file-one-bin").exists(),
        "the package's bin should be linked into the global bin directory",
    );
    let links = symlink_entries(&global_pkg_dir);
    assert_eq!(links.len(), 1, "exactly one cache-keyed hash symlink should exist: {links:?}");

    // list -g --parseable
    let output = global_command(&workspace, &pnpm_home)
        .with_arg("list")
        .with_arg("-g")
        .with_arg("--parseable")
        .output()
        .expect("run list -g");
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("list -g --parseable:\n{stdout}");
    assert!(stdout.contains("touch-file-one-bin"), "list -g should report the installed package");

    // remove -g
    global_command(&workspace, &pnpm_home)
        .with_arg("remove")
        .with_arg("-g")
        .with_arg("@foo/touch-file-one-bin")
        .assert()
        .success();

    assert!(
        !global_bin.join("touch-file-one-bin").exists(),
        "remove -g should unlink the package's bin",
    );
    assert!(
        symlink_entries(&global_pkg_dir).is_empty(),
        "remove -g should delete the hash symlink",
    );

    drop(npmrc_info);
    drop(root);
}

/// `pacquet list -g` with nothing installed reports the empty state rather
/// than erroring. No registry needed.
#[test]
fn global_list_empty() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm-home");

    let output = Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(&workspace)
        .with_env("PNPM_HOME", &pnpm_home)
        .with_arg("list")
        .with_arg("-g")
        .output()
        .expect("run list -g");

    assert!(output.status.success(), "list -g on an empty home should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No global packages found"),
        "expected the empty-state message, got: {stdout}",
    );

    drop(root);
}

/// `pacquet add -g pnpm` is rejected — pnpm is managed via `self-update`.
#[test]
fn global_add_pnpm_is_rejected() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm-home");
    fs::create_dir_all(pnpm_home.join("bin")).expect("create global bin dir");

    let output = Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(&workspace)
        .with_env("PNPM_HOME", &pnpm_home)
        .with_arg("add")
        .with_arg("-g")
        .with_arg("pnpm")
        .output()
        .expect("run add -g pnpm");

    assert!(!output.status.success(), "add -g pnpm must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("self-update"),
        "the failure should point at self-update, got: {stderr}",
    );

    drop(root);
}
