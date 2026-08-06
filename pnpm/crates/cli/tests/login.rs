//! `pacquet login` / `pacquet adduser` — the `LoginArgs::run` adapter.
//!
//! These tests drive the real command end-to-end through dispatch —
//! config-directory resolution, the `ThrottledClient` construction, and the
//! `login` call — against a local mock registry.
//!
//! The non-interactive guard lives on the CLASSIC (username / password)
//! fallback, the only path that prompts on the terminal. A spawned `pacquet`
//! process has no controlling TTY, so:
//!
//! - against a registry WITHOUT web-login support (404 on `POST /-/v1/login`)
//!   the classic fallback is reached and the
//!   `ERR_PNPM_LOGIN_NON_INTERACTIVE` diagnostic must propagate out of `run`;
//! - against a registry WITH web-login support the flow must complete without
//!   a terminal: print the authentication URL and poll the done endpoint for
//!   the granted token.
//!
//! `XDG_CONFIG_HOME` is pinned to a temp directory so a config directory
//! always resolves (past `run`'s `NoConfigDir` guard).

use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;

/// Spawn `pacquet <subcommand>` without a TTY against a classic-only registry
/// (web login probe answers 404) and assert the non-interactive login
/// diagnostic propagates from the classic fallback.
fn assert_rejects_non_interactive_terminal(subcommand: &str) {
    let mut server = mockito::Server::new();
    let login_probe = server.mock("POST", "/-/v1/login").with_status(404).create();
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_env("XDG_CONFIG_HOME", root.path())
        .with_arg(subcommand)
        .with_arg("--registry")
        .with_arg(format!("{}/", server.url()))
        .output()
        .unwrap_or_else(|error| panic!("spawn pacquet {subcommand}: {error}"));

    assert!(
        !output.status.success(),
        "`pacquet {subcommand}` must fail without a TTY on a classic-only registry (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_LOGIN_NON_INTERACTIVE")
            && stderr.contains("requires an interactive terminal"),
        "stderr must name the non-interactive diagnostic; got:\n{stderr}",
    );
    login_probe.assert();
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

/// Spawn `pacquet login` without a TTY against a registry WITH web-login
/// support and assert the headless web flow completes: the authentication URL
/// is printed and the done endpoint's token is accepted.
#[test]
fn login_completes_the_web_flow_without_a_terminal() {
    let mut server = mockito::Server::new();
    let registry = server.url();
    let done = server
        .mock("GET", "/-/v1/done")
        .with_status(200)
        .with_body(r#"{"token":"cli-headless-token"}"#)
        .create();
    let login = server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(format!(
            r#"{{"loginUrl":"{registry}/auth/login","doneUrl":"{registry}/-/v1/done"}}"#,
        ))
        .create();
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_env("XDG_CONFIG_HOME", root.path())
        .with_arg("login")
        .with_arg("--registry")
        .with_arg(format!("{registry}/"))
        .output()
        .unwrap_or_else(|error| panic!("spawn pacquet login: {error}"));

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success(),
        "`pacquet login` must complete the web flow without a TTY (stdout: {stdout})(stderr: {stderr})",
    );
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains(&format!("{registry}/auth/login")),
        "the authentication URL must be printed for the human to open; got:\n{combined}",
    );
    login.assert();
    done.assert();
    drop(root);
}

/// A project `pnpm-workspace.yaml` cannot choose the scope `pnpm login`
/// persists as a `@scope:registry` route in the global `auth.ini`, and the
/// user is told the setting was dropped rather than left to wonder why their
/// token came back unscoped. Spawns the real binary because the warning's
/// whole point is that it reaches stderr.
/// See <https://github.com/pnpm/pnpm/issues/13557>.
#[test]
fn a_workspace_yaml_scope_is_ignored_and_reported_on_stderr() {
    let mut server = mockito::Server::new();
    let registry = server.url();
    let done = server
        .mock("GET", "/-/v1/done")
        .with_status(200)
        .with_body(r#"{"token":"cli-headless-token"}"#)
        .create();
    let login = server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(format!(
            r#"{{"loginUrl":"{registry}/auth/login","doneUrl":"{registry}/-/v1/done"}}"#,
        ))
        .create();
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    std::fs::write(workspace.join("pnpm-workspace.yaml"), "scope: '@acme'\n")
        .expect("write pnpm-workspace.yaml");

    let output = pacquet
        .with_env("XDG_CONFIG_HOME", root.path())
        .with_arg("login")
        .with_arg("--registry")
        .with_arg(format!("{registry}/"))
        .output()
        .expect("spawn pacquet login");

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "`pacquet login` must still succeed; stderr:\n{stderr}");
    assert_eq!(
        stderr.matches(pacquet_config::IGNORED_SCOPE_WARNING).count(),
        1,
        "stderr must carry the ignored-scope warning verbatim, exactly once; got:\n{stderr}",
    );
    let auth_ini = std::fs::read_to_string(root.path().join("pnpm").join("auth.ini"))
        .expect("login writes auth.ini");
    assert!(
        !auth_ini.contains("@acme"),
        "the repo scope must not reach auth.ini; got:\n{auth_ini}",
    );
    login.assert();
    done.assert();
    drop(root);
}
