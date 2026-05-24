use crate::{port_to_url::port_to_url, registry_mock_storage, workspace_root};
use std::env;
use std::path::PathBuf;
use std::process::Command;

/// Locate the `pnpm-registry` binary built into the cargo workspace's
/// `target/<profile>/` dir.
///
/// `assert_cmd::cargo::cargo_bin` (the obvious choice) panics here:
/// it relies on `CARGO_BIN_EXE_<name>`, which cargo only injects
/// when running an *integration test* of the crate that defines the
/// binary. Both `pacquet-registry-mock launch` (the recipe invoked
/// from `just registry-mock launch`) and the auto-spawn path in
/// pacquet's install tests run from a different crate, so the env
/// var is unset and `cargo_bin` aborts. We resolve the path
/// ourselves:
///
/// 1. Honor `CARGO_BIN_EXE_pnpm-registry` if set — this is the case
///    inside pnpm-registry's own integration tests.
/// 2. Fall back to `$CARGO_TARGET_DIR/<profile>/pnpm-registry`,
///    defaulting to `<workspace_root>/target` when the env var isn't
///    set. `<profile>` matches the build profile of the caller via
///    `cfg!(debug_assertions)`.
fn pnpm_registry_binary() -> PathBuf {
    if let Some(path) = env::var_os("CARGO_BIN_EXE_pnpm-registry") {
        return PathBuf::from(path);
    }
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"));
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    target_dir.join(profile).join(format!("pnpm-registry{}", env::consts::EXE_SUFFIX))
}

/// Build a [`Command`] that spawns the `pnpm-registry` binary in
/// static-serve mode against `@pnpm/registry-mock`'s prepared storage
/// directory.
///
/// `pnpm-registry` is a workspace crate; run
/// `cargo build -p pnpm-registry` once before invoking the mock if
/// it isn't already built.
///
/// `--public-url` is pinned to `http://localhost:<port>` so the
/// tarball URLs the registry rewrites match the URL pacquet's tests
/// expect via [`port_to_url`](crate::port_to_url::port_to_url).
pub fn pnpm_registry_command(port: u16) -> Command {
    let bin = pnpm_registry_binary();
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
