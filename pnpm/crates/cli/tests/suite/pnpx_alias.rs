//! Covers the `current_exe`-based `dlx` injection (`argv_with_alias_subcommand`)
//! that pnpm relies on for the Windows `pnpx`/`pnx` hardlinks, and the
//! re-invocation that must not re-enter it. The binary is copied under the
//! alias name rather than linked, which reaches the same code on every
//! platform.

use std::{
    env::consts::EXE_SUFFIX,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use pnpm_testing_utils::bin::CommandTempCwd;
use tempfile::TempDir;

#[test]
fn launched_as_pnpx_injects_the_dlx_subcommand() {
    let pacquet = env!("CARGO_BIN_EXE_pnpm");

    let dir = TempDir::new().expect("create temp dir");
    let pnpx = copy_executable(Path::new(pacquet), dir.path(), "pnpx");

    let via_pnpx = Command::new(&pnpx)
        .arg("--help")
        .output()
        .expect("run `pnpx --help`");
    let via_dlx = Command::new(pacquet)
        .args(["dlx", "--help"])
        .output()
        .expect("run `dlx --help`");

    assert!(via_pnpx.status.success(), "`pnpx --help` exited with a failure status");
    assert!(via_dlx.status.success(), "`dlx --help` (the control) exited with a failure status");
    let pnpx_help = String::from_utf8(via_pnpx.stdout).expect("pnpx help is UTF-8");
    let dlx_help = String::from_utf8(via_dlx.stdout).expect("dlx help is UTF-8");
    assert_eq!(pnpx_help, dlx_help, "`pnpx --help` should equal `dlx --help` (dlx was injected)");
}

#[test]
fn launched_as_pnpx_hands_scripts_the_pnpm_beside_it() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let bin_dir = workspace.join("bin");
    fs::create_dir(&bin_dir).expect("create bin directory");
    let pnpm = copy_executable(Path::new(pacquet.get_program()), &bin_dir, "pnpm");
    let pnpx = copy_executable(Path::new(pacquet.get_program()), &bin_dir, "pnpx");
    let fixture = workspace.join("fixture");
    fs::create_dir(&fixture).expect("create package fixture");
    fs::write(fixture.join("package.json"), r#"{"name":"execpath-fixture","version":"1.0.0"}"#)
        .expect("write package manifest");
    let node = which::which("node").expect("find node");

    let mut via_pnpx = Command::new(&pnpx);
    via_pnpx.current_dir(pacquet.get_current_dir().unwrap_or(&workspace));
    for (key, value) in pacquet.get_envs() {
        match value {
            Some(value) => via_pnpx.env(key, value),
            None => via_pnpx.env_remove(key),
        };
    }
    let output = via_pnpx
        .env_remove("npm_execpath")
        .arg(format!("--package=file:{}", fixture.display()))
        .arg(node)
        .args(["-e", "console.log(process.env.npm_execpath)"])
        .output()
        .expect("run pnpx");

    assert!(
        output.status.success(),
        "pnpx exited with a failure status: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let execpath = String::from_utf8(output.stdout).expect("npm_execpath is UTF-8");
    assert_eq!(
        fs::canonicalize(execpath.trim()).expect("resolve npm_execpath"),
        fs::canonicalize(&pnpm).expect("resolve the pnpm copy"),
    );
    drop(root);
}

/// Copies `source` into `dir` as the executable `name`. Windows has no
/// executable bit, so the `.exe` suffix is what makes the copy runnable there.
fn copy_executable(source: &Path, dir: &Path, name: &str) -> PathBuf {
    let target = dir.join(format!("{name}{EXE_SUFFIX}"));
    fs::copy(source, &target).expect("copy the pnpm binary");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755))
            .expect("make the copy executable");
    }
    target
}
