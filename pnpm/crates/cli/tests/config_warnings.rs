//! Config-load warnings reach stderr once per command.
//!
//! pnpm reads config once per command and prints its warning list once.
//! pacquet loads `Config` lazily and sometimes more than once — `install`
//! consults the up-to-date fast path with its own load before `run` builds a
//! second one off the same files — so the emit-once rule is a property of the
//! command, not of a single `Config`. Only spawning the binary exercises that.

use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;

/// `install` is the command that loads config twice: the fast path bails here
/// (no lockfile, no `node_modules`), leaving `run` to load it again.
#[test]
fn install_reports_an_ignored_workspace_scope_exactly_once() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    std::fs::write(workspace.join("package.json"), r#"{"name":"w","version":"1.0.0"}"#)
        .expect("write package.json");
    std::fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - '.'\nscope: '@acme'\n")
        .expect("write pnpm-workspace.yaml");

    let output = pacquet
        .with_env("XDG_CONFIG_HOME", root.path())
        .with_arg("install")
        .with_arg("--offline")
        .output()
        .expect("spawn pacquet install");

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(
        stderr.matches(pacquet_config::IGNORED_SCOPE_WARNING).count(),
        1,
        "the ignored-scope warning must appear once, not once per config load; got:\n{stderr}",
    );
    drop(root);
}
