use crate::{port_to_url::port_to_url, registry_mock_storage};
use assert_cmd::cargo::cargo_bin;
use std::process::Command;

/// Build a [`Command`] that spawns the `pnpm-registry` binary in
/// static-serve mode against `@pnpm/registry-mock`'s prepared storage
/// directory.
///
/// `pnpm-registry` is a workspace crate; `cargo_bin` finds the built
/// binary under the shared `target/<profile>/` dir (run
/// `cargo build -p pnpm-registry` once before invoking tests if it
/// isn't already built).
///
/// `--public-url` is pinned to `http://localhost:<port>` so the
/// tarball URLs the registry rewrites match the URL pacquet's tests
/// expect via [`port_to_url`](crate::port_to_url::port_to_url).
pub fn pnpm_registry_command(port: u16) -> Command {
    let bin = cargo_bin("pnpm-registry");
    assert!(
        bin.is_file(),
        "pnpm-registry binary not found at {bin:?} — \
         run `cargo build -p pnpm-registry` before invoking the mock",
    );
    let mut cmd = Command::new(bin);
    cmd.arg("--static")
        .arg("--storage")
        .arg(registry_mock_storage())
        .arg("--listen")
        .arg(format!("127.0.0.1:{port}"))
        .arg("--public-url")
        .arg(port_to_url(port).trim_end_matches('/'));
    cmd
}
