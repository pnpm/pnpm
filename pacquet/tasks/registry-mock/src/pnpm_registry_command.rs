use crate::{
    port_to_url::port_to_url, runtime_storage, seed_storage::seed_runtime_storage, workspace_root,
};
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
/// proxy mode against `@pnpm/registry-mock`'s prepared storage
/// directory, with `registry.npmjs.org` as the upstream.
///
/// This mirrors verdaccio's `'**': proxy: npmjs` setup in
/// `@pnpm/registry-mock`'s `registry/config.yaml`: the storage
/// holds the scoped fixture packages (`@foo`, `@pnpm.e2e`, …) and
/// anything else (e.g. `is-positive`, `json-append`) is pulled
/// from npmjs.org on demand.
///
/// `--packument-ttl-secs 31536000` (one year) keeps the fixture
/// packuments authoritative across a test run: their on-disk
/// mtime is whenever the npm tarball was built, which is far older
/// than any sane TTL, so a short TTL would mark them stale and try
/// to refetch from npm — where the fixtures don't exist — and 404.
///
/// `pnpm-registry` is a workspace crate; run
/// `cargo build -p pnpm-registry` once before invoking the mock if
/// it isn't already built. `--public-url` is pinned to
/// `http://localhost:<port>` so the tarball URLs the registry
/// rewrites match the URL pacquet's tests expect via
/// `port_to_url`.
pub fn pnpm_registry_command(port: u16) -> Command {
    let bin = pnpm_registry_binary();
    assert!(
        bin.is_file(),
        "pnpm-registry binary not found at {bin:?} — \
         run `cargo build -p pnpm-registry` before invoking the mock",
    );
    // Seed the runtime storage with the registry-mock fixtures
    // before pnpm-registry starts serving. Idempotent — existing
    // files are left alone, so CI can cache the runtime path
    // across runs and only npm-proxied entries get fetched fresh.
    let seeded = seed_runtime_storage()
        .unwrap_or_else(|err| panic!("seed registry-mock fixtures into runtime storage: {err}"));
    if seeded > 0 {
        eprintln!("info: seeded {seeded} fixture file(s) into runtime storage");
    }
    let mut cmd = Command::new(bin);
    cmd.arg("--storage")
        .arg(runtime_storage())
        .arg("--upstream")
        .arg("https://registry.npmjs.org")
        .arg("--packument-ttl-secs")
        .arg("31536000")
        .arg("--listen")
        .arg(format!("127.0.0.1:{port}"))
        .arg("--public-url")
        .arg(port_to_url(port).trim_end_matches('/'));
    cmd
}
