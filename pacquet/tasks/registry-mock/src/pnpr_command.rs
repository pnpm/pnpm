use crate::{
    port_to_url::port_to_url, runtime_storage, seed_storage::seed_runtime_storage, workspace_root,
};
use std::{env, path::PathBuf, process::Command};

/// Locate the `pnpr` binary built into the cargo workspace's
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
/// 1. Honor `CARGO_BIN_EXE_pnpr` if set — this is the case
///    inside pnpr's own integration tests.
/// 2. Prefer the release binary if one exists at
///    `$CARGO_TARGET_DIR/release/pnpr`. Critical for the
///    integrated benchmark: comparing a debug-Rust mock to a
///    JIT-optimized verdaccio is apples to oranges — the install
///    measures 20%+ slower purely from unoptimized JSON parse +
///    serialize on every request. Tests don't need the release
///    binary, but having a release build override the debug one is
///    always the right choice when both exist.
/// 3. Fall back to `$CARGO_TARGET_DIR/debug/pnpr` for
///    local dev where only `cargo build` ran.
fn pnpr_binary() -> PathBuf {
    if let Some(path) = env::var_os("CARGO_BIN_EXE_pnpr") {
        return PathBuf::from(path);
    }
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| workspace_root().join("target"), PathBuf::from);
    let exe = format!("pnpr{}", env::consts::EXE_SUFFIX);
    let release = target_dir.join("release").join(&exe);
    if release.is_file() {
        return release;
    }
    target_dir.join("debug").join(&exe)
}

/// Build a [`Command`] that spawns the `pnpr` binary in
/// proxy mode against the storage built from the in-repo fixtures,
/// with `registry.npmjs.org` as the upstream.
///
/// The storage holds the fixture packages (`@foo`, `@pnpm.e2e`, ...)
/// and anything else (e.g. extra npm packages the benchmark lockfile
/// pulls) is fetched from npmjs.org on demand.
///
/// `--packument-ttl-secs 31536000` (one year) keeps the fixture
/// packuments authoritative across a test run: their on-disk
/// mtime is whenever the npm tarball was built, which is far older
/// than any sane TTL, so a short TTL would mark them stale and try
/// to refetch from npm — where the fixtures don't exist — and 404.
///
/// `pnpr` is a workspace crate; run
/// `cargo build -p pnpr` once before invoking the mock if
/// it isn't already built. `--public-url` defaults to
/// `http://localhost:<port>` so the tarball URLs the registry rewrites
/// match the URL pacquet's tests expect via `port_to_url`. The
/// integrated benchmark can override it when a proxy fronts the registry
/// port: packuments served by pnpr must advertise the proxy URL, or
/// tarball downloads bypass the emulated registry link.
#[must_use]
pub fn pnpr_command(port: u16, public_url: Option<&str>) -> Command {
    let bin = pnpr_binary();
    assert!(
        bin.is_file(),
        "pnpr binary not found at {bin:?} — \
         run `cargo build -p pnpr` before invoking the mock",
    );
    // Seed the runtime storage with the registry-mock fixtures
    // before pnpr starts serving. Idempotent — existing
    // files are left alone, so CI can cache the runtime path
    // across runs and only npm-proxied entries get fetched fresh.
    let seeded = seed_runtime_storage()
        .unwrap_or_else(|err| panic!("seed registry-mock fixtures into runtime storage: {err}"));
    if seeded > 0 {
        eprintln!("info: seeded {seeded} fixture file(s) into runtime storage");
    }
    let default_public_url;
    let public_url = if let Some(public_url) = public_url {
        public_url.trim_end_matches('/')
    } else {
        default_public_url = port_to_url(port);
        default_public_url.trim_end_matches('/')
    };
    let mut cmd = Command::new(bin);
    // `pnpr` defaults to its bundled verdaccio-shaped config
    // (npmjs uplink + `**` proxy rule), which matches what the mock
    // needs — no `-c` override required. We only pin the runtime
    // bits the bundled config can't know about.
    cmd.arg("--storage")
        .arg(runtime_storage())
        .arg("--packument-ttl-secs")
        .arg("31536000")
        .arg("--listen")
        .arg(format!("127.0.0.1:{port}"))
        .arg("--public-url")
        .arg(public_url);
    cmd
}
