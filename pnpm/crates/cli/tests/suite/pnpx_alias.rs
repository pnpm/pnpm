//! Covers the `current_exe`-based `dlx` injection (`argv_with_alias_subcommand`)
//! that pnpm relies on for the Windows `pnpx`/`pnx` hardlinks. The binary is
//! copied under the alias name rather than linked, which reaches the same code
//! on every platform.

use std::{
    env::consts::EXE_SUFFIX,
    fs,
    process::Command,
};

use tempfile::TempDir;

#[test]
fn launched_as_pnpx_injects_the_dlx_subcommand() {
    let pacquet = env!("CARGO_BIN_EXE_pnpm");

    let dir = TempDir::new().expect("create temp dir");
    let pnpx = dir
        .path()
        .join(format!("pnpx{EXE_SUFFIX}"));
    fs::copy(pacquet, &pnpx).expect("copy the binary under the pnpx name");
    // Windows has no executable bit — the `.exe` suffix above is what
    // makes the copy runnable there.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&pnpx, fs::Permissions::from_mode(0o755))
            .expect("make pnpx executable");
    }

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
