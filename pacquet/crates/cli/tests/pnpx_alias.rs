//! The `pnpx` / `pnx` bins are the native binary shipped under another name; the
//! binary detects its launch name via `current_exe` and injects the `dlx`
//! subcommand (`argv_with_alias_subcommand` in the cli crate). This is the path
//! pnpm relies on for the Windows alias hardlinks. We exercise it on Unix by
//! copying the binary under the alias name and running it — the same
//! `current_exe`-based code runs on every platform, so Unix coverage protects
//! the Windows wiring too.
#![cfg(unix)]

use std::{fs, os::unix::fs::PermissionsExt, process::Command};

use tempfile::TempDir;

/// Invoking the binary as `pnpx` must behave like `pnpm dlx`: `pnpx --help`
/// renders the exact same help as `dlx --help`.
#[test]
fn launched_as_pnpx_injects_the_dlx_subcommand() {
    let pacquet = env!("CARGO_BIN_EXE_pacquet");

    let dir = TempDir::new().expect("create temp dir");
    let pnpx = dir.path().join("pnpx");
    fs::copy(pacquet, &pnpx).expect("copy the binary under the pnpx name");
    fs::set_permissions(&pnpx, fs::Permissions::from_mode(0o755)).expect("make pnpx executable");

    let via_pnpx = Command::new(&pnpx).arg("--help").output().expect("run `pnpx --help`");
    let via_dlx = Command::new(pacquet).args(["dlx", "--help"]).output().expect("run `dlx --help`");

    assert!(via_pnpx.status.success(), "`pnpx --help` exited with a failure status");
    assert!(via_dlx.status.success(), "`dlx --help` (the control) exited with a failure status");
    let pnpx_help = String::from_utf8(via_pnpx.stdout).expect("pnpx help is UTF-8");
    let dlx_help = String::from_utf8(via_dlx.stdout).expect("dlx help is UTF-8");
    assert_eq!(pnpx_help, dlx_help, "`pnpx --help` should equal `dlx --help` (dlx was injected)");
}
