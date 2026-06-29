use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use pretty_assertions::assert_eq;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Canonicalize a path the way the production CLI does (`dunce::canonicalize`
/// on `--dir`), so the expected value matches the resolved form pacquet prints
/// — e.g. on macOS a `/var/folders/...` temp dir surfaces as `/private/var/...`.
fn canonicalize(path: &Path) -> PathBuf {
    dunce::canonicalize(path).expect("canonicalize path")
}

#[test]
fn bin_prints_the_local_node_modules_bin_dir() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    let output = pacquet.with_args(["bin"]).output().expect("run pacquet bin");
    dbg!(&output);
    assert!(output.status.success(), "pacquet bin should succeed");

    let expected =
        format!("{}\n", canonicalize(&workspace).join("node_modules").join(".bin").display());
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);

    drop(root);
}

#[test]
fn bin_ignores_a_custom_modules_dir() {
    // pnpm hardcodes the `.bin` leaf, so a custom modules-dir is ignored.
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "modulesDir: custom_nm\n")
        .expect("write pnpm-workspace.yaml");

    let output = pacquet.with_args(["bin"]).output().expect("run pacquet bin");
    dbg!(&output);
    assert!(output.status.success(), "pacquet bin should succeed");

    let expected =
        format!("{}\n", canonicalize(&workspace).join("node_modules").join(".bin").display());
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);

    drop(root);
}

/// `pacquet bin -g` resolves, creates, and (matching pnpm) validates the global
/// bin dir is on `PATH` before printing. The env is pinned so the resolved path
/// is deterministic. Unix-gated like `global.rs`: the `PATH` validation is
/// platform-specific.
#[cfg(unix)]
#[test]
fn bin_global_prints_the_global_bin_dir_when_on_path() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    let existing_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{existing_path}", global_bin.display());

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("HOME", root.path())
        .with_env("XDG_CONFIG_HOME", root.path().join("xdg-config"))
        .with_env("PNPM_CONFIG_GLOBAL_BIN_DIR", "")
        .with_env("pnpm_config_global_bin_dir", "")
        .with_env("PATH", path)
        .with_args(["bin", "-g"])
        .output()
        .expect("run pacquet bin -g");
    dbg!(&output);
    assert!(output.status.success(), "pacquet bin -g should succeed when the dir is on PATH");

    let expected = format!("{}\n", global_bin.display());
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    assert!(global_bin.is_dir(), "pacquet bin -g should create the global bin dir");

    drop(root);
}

/// `pacquet bin -g` errors like pnpm (`ERR_PNPM_GLOBAL_BIN_DIR_NOT_IN_PATH`)
/// when the resolved global bin directory is not on `PATH`. Unix-gated for the
/// same `PATH`-format reason as the success case.
#[cfg(unix)]
#[test]
fn bin_global_errors_when_not_in_path() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm-home");

    let output = pacquet
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("HOME", root.path())
        .with_env("XDG_CONFIG_HOME", root.path().join("xdg-config"))
        .with_env("PNPM_CONFIG_GLOBAL_BIN_DIR", "")
        .with_env("pnpm_config_global_bin_dir", "")
        .with_env("PATH", "/usr/bin:/bin")
        .with_args(["bin", "-g"])
        .output()
        .expect("run pacquet bin -g");
    dbg!(&output);
    assert!(!output.status.success(), "pacquet bin -g should fail when the dir is not on PATH");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not in PATH") || stderr.contains("ERR_PNPM_GLOBAL_BIN_DIR_NOT_IN_PATH"),
        "stderr should explain the global bin dir is not in PATH: {stderr}",
    );

    drop(root);
}

/// Differential parity: from a workspace subdirectory pnpm's `bin` prints the
/// cwd's `node_modules/.bin` (its `config.dir` is the cwd, not the workspace
/// root). pacquet must match byte-for-byte. Windows-skipped because it spawns
/// the external `pnpm` shim (see the `ignore` reason).
#[test]
#[cfg_attr(
    target_os = "windows",
    ignore = "spawns the external `pnpm` shim (`pnpm.cmd`); std::process::Command can't resolve it via PATHEXT"
)]
fn bin_matches_pnpm_from_a_workspace_subdir() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();

    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - \"packages/*\"\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("package.json"), r#"{ "name": "wsroot", "version": "1.0.0" }"#)
        .expect("write workspace-root package.json");
    let member = workspace.join("packages/foo");
    fs::create_dir_all(&member).expect("create workspace member dir");
    fs::write(member.join("package.json"), r#"{ "name": "foo", "version": "1.0.0" }"#)
        .expect("write member package.json");

    let pacquet_out = Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(&member)
        .with_args(["bin"])
        .output()
        .expect("run pacquet bin in the subdir");
    assert!(pacquet_out.status.success(), "pacquet bin should succeed in the subdir");

    let pnpm_out = Command::new("pnpm")
        .with_current_dir(&member)
        .with_args(["bin"])
        .output()
        .expect("run pnpm bin in the subdir");
    assert!(
        pnpm_out.status.success(),
        "pnpm bin failed: {}",
        String::from_utf8_lossy(&pnpm_out.stderr),
    );

    let pacquet_stdout = String::from_utf8_lossy(&pacquet_out.stdout);
    let pnpm_stdout = String::from_utf8_lossy(&pnpm_out.stdout);
    eprintln!("pacquet: {pacquet_stdout:?}\npnpm:    {pnpm_stdout:?}");
    assert_eq!(
        pacquet_stdout, pnpm_stdout,
        "pacquet bin must match pnpm bin from a workspace subdir (cwd, not workspace root)",
    );

    drop(root);
}
