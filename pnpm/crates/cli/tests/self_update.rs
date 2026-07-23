//! `pacquet self-update` — dispatch-level coverage.
//!
//! Exercises the dispatch wiring that routes `self-update` through the
//! `config_self_update` closure, which resolves the release-age policy so a
//! repo-controlled `pnpm-workspace.yaml` can only tighten it. The resolver can't be covered here because
//! the mock registry doesn't serve `pnpm` / `@pnpm/exe` (see `tests/with.rs`
//! for the same limitation), so the command is expected to fail at the
//! resolve step — by which point the closure has already run.
//!
//! The global pnpm install is isolated by pointing `PNPM_HOME` at a temp
//! dir, so even if a future edit changes the version to a resolvable one,
//! `link_into_global_bin` writes into the temp dir instead of clobbering the
//! caller's real pnpm. The `999.999.999` version is the belt to that
//! suspenders: it doesn't resolve, so the link step never runs at all.

use std::fs;

use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use tempfile::tempdir;

#[test]
fn self_update_loads_config_and_reaches_the_resolver() {
    let CommandTempCwd { mut pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    // Isolate the global pnpm install: `PNPM_HOME` drives `global_pkg_dir` /
    // `global_bin` / the store, so any link step writes into this temp dir
    // rather than the caller's real pnpm home.
    let global_home = tempdir().expect("global home tempdir");
    pacquet.env("PNPM_HOME", global_home.path());

    // A workspace `minimumReleaseAge` is present to mirror a realistic
    // invocation; it isn't load-bearing for this test's assertions because
    // the resolve fails before any maturity check runs. The policy resolution
    // itself is covered by the config-crate unit tests.
    fs::write(workspace.join("pnpm-workspace.yaml"), "minimumReleaseAge: 1440\n")
        .expect("write pnpm-workspace.yaml");

    let output = pacquet
        .with_args(["self-update", "999.999.999"])
        .output()
        .expect("run pacquet self-update");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("Checking for updates") || stderr.contains("Checking for updates"),
        "the self-update closure should load config and reach the handler; stdout={stdout}, stderr={stderr}",
    );
    assert!(
        !output.status.success(),
        "self-update should fail at the resolve step because `999.999.999` is not on the mock registry",
    );

    drop(root);
    drop(global_home);
}
