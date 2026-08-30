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
use pipe_trait::Pipe;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::fs;

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

/// A scoped login records the token under `_auth` and routes the scope to the
/// registry under `registries`, both in the global `config.yaml` — the two
/// settings the reader accepts from a file no repository can write. Spawns the
/// real binary because the production writer, not the `Sys` fake, is what has
/// to produce a document the reader accepts back.
#[test]
fn a_scoped_login_records_the_token_and_route_in_config_yaml() {
    let mut server = mockito::Server::new();
    let registry = server.url();
    let done = server
        .mock("GET", "/-/v1/done")
        .with_status(200)
        .with_body(serde_json::json!({ "token": "cli-scoped-token" }).to_string())
        .create();
    let login = server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "loginUrl": format!("{registry}/auth/login"),
                "doneUrl": format!("{registry}/-/v1/done"),
            })
            .to_string(),
        )
        .create();
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_env("XDG_CONFIG_HOME", root.path())
        .with_arg("login")
        .with_arg("--registry")
        .with_arg(format!("{registry}/"))
        .with_arg("--scope")
        .with_arg("@acme")
        .output()
        .expect("spawn pacquet login");

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "`pacquet login` must succeed; stderr:\n{stderr}");
    let document: serde_json::Value = root
        .path()
        .join("pnpm")
        .join("config.yaml")
        .pipe(fs::read_to_string)
        .expect("login writes config.yaml")
        .pipe_as_ref(serde_saphyr::from_str)
        .expect("login writes valid YAML");
    let normalized = format!("{registry}/");
    assert_eq!(
        document["_auth"][&normalized],
        serde_json::json!({ "@acme": { "authToken": "cli-scoped-token" } }),
    );
    assert_eq!(document["registries"][&normalized], serde_json::json!({ "scopes": ["@acme"] }));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = fs::metadata(root.path().join("pnpm").join("config.yaml"))
            .expect("stat config.yaml")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "the file now holds a token; got {mode:o}");
    }
    login.assert();
    done.assert();
    drop(root);
}

/// The document a login leaves behind is one the reader accepts back: the
/// scope resolves to the registry that was logged in to, and the token it was
/// granted stays out of `pnpm config list`.
#[test]
fn a_login_writes_a_config_the_reader_reads_back() {
    const TOKEN: &str = "round-trip-token";
    let mut server = mockito::Server::new();
    let registry = server.url();
    server
        .mock("GET", "/-/v1/done")
        .with_status(200)
        .with_body(serde_json::json!({ "token": TOKEN }).to_string())
        .create();
    server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "loginUrl": format!("{registry}/auth/login"),
                "doneUrl": format!("{registry}/-/v1/done"),
            })
            .to_string(),
        )
        .create();
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let login = pacquet
        .with_env("XDG_CONFIG_HOME", root.path())
        .with_arg("login")
        .with_arg("--registry")
        .with_arg(format!("{registry}/"))
        .with_arg("--scope")
        .with_arg("@acme")
        .output()
        .expect("spawn pacquet login");
    assert!(
        login.status.success(),
        "`pacquet login` must succeed; stderr:\n{}",
        String::from_utf8_lossy(&login.stderr),
    );

    let listed = CommandTempCwd::init()
        .pacquet
        .with_env("XDG_CONFIG_HOME", root.path())
        .with_arg("config")
        .with_arg("list")
        .output()
        .expect("spawn pacquet config list");

    let stderr = String::from_utf8_lossy(&listed.stderr).into_owned();
    assert!(listed.status.success(), "the written config must load; stderr:\n{stderr}");
    let stdout = String::from_utf8_lossy(&listed.stdout).into_owned();
    assert!(
        stdout.contains(&format!(r#""@acme:registry": "{registry}/""#)),
        "the scope must resolve to the registry logged in to; got:\n{stdout}",
    );
    assert!(!stdout.contains(TOKEN), "the token must not be listed; got:\n{stdout}");
    drop(root);
}

/// A project `pnpm-workspace.yaml` cannot choose the scope `pnpm login`
/// persists, and the user is told the setting was dropped rather than left to
/// wonder why their token came back unscoped. Spawns the real binary because
/// the warning's whole point is that it reaches stderr.
/// See <https://github.com/pnpm/pnpm/issues/13557>.
#[test]
fn a_workspace_yaml_scope_is_ignored_and_reported_on_stderr() {
    let mut server = mockito::Server::new();
    let registry = server.url();
    let done = server
        .mock("GET", "/-/v1/done")
        .with_status(200)
        .with_body(serde_json::json!({ "token": "cli-headless-token" }).to_string())
        .create();
    let login = server
        .mock("POST", "/-/v1/login")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "loginUrl": format!("{registry}/auth/login"),
                "doneUrl": format!("{registry}/-/v1/done"),
            })
            .to_string(),
        )
        .create();
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "scope: '@acme'\n")
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
        stderr
            .matches(
                r#"were ignored: "scope" (Set it for the machine instead: pnpm config set --global scope)."#
            )
            .count(),
        1,
        "stderr must name the dropped scope and where it belongs, exactly once; got:\n{stderr}",
    );
    let document = root
        .path()
        .join("pnpm")
        .join("config.yaml")
        .pipe(fs::read_to_string)
        .expect("login writes config.yaml");
    assert!(
        !document.contains("@acme"),
        "the repo scope must not reach the recorded login; got:\n{document}",
    );
    login.assert();
    done.assert();
    drop(root);
}
