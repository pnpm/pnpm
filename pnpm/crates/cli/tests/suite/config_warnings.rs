//! Config-load warnings reach stderr once per command.
//!
//! See `EMITTED_CONFIG_WARNINGS` in `cli_args::config_warnings` for why a
//! command can collect the same warning more than once. Only spawning the
//! binary crosses both of the config loads `install` performs, so the rule
//! cannot be asserted from a unit test.

use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;

/// The fast path bails here — nothing is installed to be up to date about —
/// leaving `run` to load config a second time.
#[test]
fn install_reports_an_ignored_workspace_scope_exactly_once() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    std::fs::write(workspace.join("package.json"), r#"{"name":"w","version":"1.0.0"}"#)
        .expect("write package.json");
    // `storeDir` / `cacheDir` are pinned under the temp root so the run cannot
    // reach into the real pnpm home, as `CommandTempCwd::add_mocked_registry`
    // does for the tests that need a registry.
    std::fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - '.'\nscope: '@acme'\nstoreDir: ../store\ncacheDir: ../cache\n",
    )
    .expect("write pnpm-workspace.yaml");

    let output = pacquet
        .with_env("XDG_CONFIG_HOME", root.path())
        .with_arg("install")
        .with_arg("--offline")
        .output()
        .expect("spawn pacquet install");

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    // Without this the count can hold at 1 because the command died before the
    // second config load, which is exactly the coverage this test is for.
    assert!(output.status.success(), "`pnpm install` must succeed; stderr:\n{stderr}");
    assert_eq!(
        stderr.matches(pnpm_config::IGNORED_SCOPE_WARNING).count(),
        1,
        "the ignored-scope warning must appear once, not once per config load; got:\n{stderr}",
    );
    drop(root);
}
