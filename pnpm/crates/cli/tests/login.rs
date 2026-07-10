//! `pacquet login` / `pacquet adduser` — the `LoginArgs::run` adapter.
//!
//! A spawned `pacquet` process has no controlling TTY, so `login` must reject
//! the non-interactive terminal before any network I/O. These tests drive the
//! real command end-to-end through dispatch — config-directory resolution, the
//! `ThrottledClient` construction, and the `login` call — and assert the
//! `ERR_PNPM_LOGIN_NON_INTERACTIVE` diagnostic propagates out of `run`.
//!
//! `XDG_CONFIG_HOME` is pinned to a temp directory so a config directory always
//! resolves (past `run`'s `NoConfigDir` guard), and `--registry` targets a
//! loopback port with no listener so a regression that skipped the TTY check
//! could still never reach the real npm registry.

use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;

/// Spawn `pacquet <subcommand> --registry <closed>` without a TTY and assert it
/// fails with the non-interactive login diagnostic.
fn assert_rejects_non_interactive_terminal(subcommand: &str) {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_env("XDG_CONFIG_HOME", root.path())
        .with_arg(subcommand)
        .with_arg("--registry")
        .with_arg("http://127.0.0.1:9/")
        .output()
        .unwrap_or_else(|error| panic!("spawn pacquet {subcommand}: {error}"));

    assert!(
        !output.status.success(),
        "`pacquet {subcommand}` must fail without a TTY (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_LOGIN_NON_INTERACTIVE")
            && stderr.contains("requires an interactive terminal"),
        "stderr must name the non-interactive diagnostic; got:\n{stderr}",
    );
    drop(root);
}

#[test]
fn login_rejects_a_non_interactive_terminal() {
    assert_rejects_non_interactive_terminal("login");
}

#[test]
fn adduser_alias_rejects_a_non_interactive_terminal() {
    assert_rejects_non_interactive_terminal("adduser");
}
