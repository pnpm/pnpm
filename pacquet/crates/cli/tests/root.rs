use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use pretty_assertions::assert_eq;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Canonicalize a path the way the production CLI does. The CLI runs
/// `dunce::canonicalize` on `--dir`, so the printed path is the resolved
/// form — e.g. on macOS a `/var/folders/...` temp dir surfaces as
/// `/private/var/folders/...`. Mirror that so the expected value matches.
fn canonicalize(path: &Path) -> PathBuf {
    dunce::canonicalize(path).expect("canonicalize path")
}

#[test]
fn root_prints_the_local_node_modules_dir() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    let output = pacquet.with_args(["root"]).output().expect("run pacquet root");
    dbg!(&output);
    assert!(output.status.success(), "pacquet root should succeed");

    // Deliberately not trimmed, unlike `store path` — pnpm's `root` handler
    // emits the path with its trailing newline (`${path}\n`).
    let expected = format!("{}\n", canonicalize(&workspace).join("node_modules").display());
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);

    drop(root);
}

#[test]
fn root_ignores_a_custom_modules_dir() {
    // pnpm's `root` hardcodes the `node_modules` leaf, so a configured
    // modules-dir must NOT change its output. pacquet matches by anchoring on
    // `--dir` and never reading `config.modules_dir` in this command.
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "modulesDir: custom_nm\n")
        .expect("write pnpm-workspace.yaml");

    let output = pacquet.with_args(["root"]).output().expect("run pacquet root");
    dbg!(&output);
    assert!(output.status.success(), "pacquet root should succeed");

    let expected = format!("{}\n", canonicalize(&workspace).join("node_modules").display());
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);

    drop(root);
}

/// `--global` / `-g` is rejected until global package management is ported.
#[test]
fn root_global_is_not_supported_yet() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet.with_args(["root", "-g"]).output().expect("run pacquet root -g");
    dbg!(&output);
    assert!(!output.status.success(), "pacquet root -g should fail until global support lands");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not supported yet"), "stderr should explain the gap: {stderr}");

    drop(root);
}

/// Differential parity: from a workspace subdirectory pnpm's `root` prints the
/// cwd's `node_modules` (its `config.dir` is the cwd, not the workspace root).
/// pacquet must print byte-identical output.
///
/// Skipped on Windows, where pnpm is installed as a `pnpm.cmd` shim and
/// `std::process::Command` does not honor `PATHEXT`, so `Command::new("pnpm")`
/// fails with "program not found" (the same reason `pnpm_compatibility.rs` and
/// `hoist.rs` gate their pnpm-spawning tests). The three tests above spawn only
/// `pacquet`, so they keep running on Windows.
#[test]
#[cfg_attr(
    target_os = "windows",
    ignore = "spawns the external `pnpm` shim (`pnpm.cmd`); std::process::Command can't resolve it via PATHEXT"
)]
fn root_matches_pnpm_from_a_workspace_subdir() {
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
        .with_args(["root"])
        .output()
        .expect("run pacquet root in the subdir");
    assert!(pacquet_out.status.success(), "pacquet root should succeed in the subdir");

    let pnpm_out = Command::new("pnpm")
        .with_current_dir(&member)
        .with_args(["root"])
        .output()
        .expect("run pnpm root in the subdir");
    assert!(
        pnpm_out.status.success(),
        "pnpm root failed: {}",
        String::from_utf8_lossy(&pnpm_out.stderr),
    );

    let pacquet_stdout = String::from_utf8_lossy(&pacquet_out.stdout);
    let pnpm_stdout = String::from_utf8_lossy(&pnpm_out.stdout);
    eprintln!("pacquet: {pacquet_stdout:?}\npnpm:    {pnpm_stdout:?}");
    assert_eq!(
        pacquet_stdout, pnpm_stdout,
        "pacquet root must match pnpm root from a workspace subdir (cwd, not workspace root)",
    );

    drop(root);
}
