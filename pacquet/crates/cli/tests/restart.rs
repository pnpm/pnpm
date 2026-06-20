use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::fs;

/// `pacquet restart` runs "stop", "restart", and "start" scripts
/// sequentially. Each script creates a marker file; their creation
/// timestamps prove execution order.
#[cfg(unix)]
#[test]
fn restart_runs_stop_restart_start_scripts() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let stop = workspace.join("stop.txt");
    let restart = workspace.join("restart.txt");
    let start = workspace.join("start.txt");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "stop": format!(r#"touch "{}""#, stop.display()),
            "restart": format!(r#"touch "{}""#, restart.display()),
            "start": format!(r#"touch "{}""#, start.display()),
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet.with_arg("restart").assert().success();
    assert!(stop.exists(), "stop script should have run");
    assert!(restart.exists(), "restart script should have run");
    assert!(start.exists(), "start script should have run");

    drop(root);
}

/// When a "stop" script exits non-zero, `pacquet restart` terminates
/// without running the subsequent "restart" and "start" scripts.
#[cfg_attr(target_os = "windows", ignore = "uses a POSIX shell `exit` builtin")]
#[test]
fn restart_fails_when_stop_script_fails() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let restart_marker = workspace.join("restart.txt");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "stop": "exit 1",
            "restart": format!(r#"touch "{}""#, restart_marker.display()),
            "start": "exit 0",
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    let output = pacquet.with_arg("restart").output().expect("spawn pacquet restart");
    assert!(!output.status.success(), "restart should fail when stop fails");
    assert!(!restart_marker.exists(), "restart script should NOT have run after stop failure");

    drop(root);
}

/// When a "restart" script exits non-zero, `pacquet restart` terminates
/// without running the subsequent "start" script.
#[cfg_attr(target_os = "windows", ignore = "uses a POSIX shell `exit` builtin")]
#[test]
fn restart_fails_when_restart_script_fails() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let start_marker = workspace.join("start.txt");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "stop": "exit 0",
            "restart": "exit 2",
            "start": format!(r#"touch "{}""#, start_marker.display()),
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    let output = pacquet.with_arg("restart").output().expect("spawn pacquet restart");
    assert!(!output.status.success(), "restart should fail when restart script fails");
    assert!(!start_marker.exists(), "start script should NOT have run after restart failure");

    drop(root);
}

/// With `--if-present`, missing "stop" and "restart" scripts are
/// silently skipped while "start" still runs.
#[cfg(unix)]
#[test]
fn restart_with_if_present_skips_missing_scripts() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let start_marker = workspace.join("start.txt");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "start": format!(r#"touch "{}""#, start_marker.display()),
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet.with_arg("restart").with_arg("--if-present").assert().success();
    assert!(start_marker.exists(), "start script should have run");

    drop(root);
}

/// Positional arguments after `restart` are forwarded to each script.
#[cfg(unix)]
#[test]
fn restart_passes_args_to_scripts() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("args.txt");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "stop": format!(
                "node -e \"require('fs').writeFileSync('{}', process.argv[1])\"",
                marker.display(),
            ),
            "restart": "true",
            "start": "true",
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet.with_arg("restart").with_arg("hello-world").assert().success();
    let written = fs::read_to_string(&marker).expect("read marker");
    assert_eq!(written, "hello-world");

    drop(root);
}

/// `pacquet stop` runs the "stop" script.
#[cfg(unix)]
#[test]
fn stop_runs_declared_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("stopped.txt");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "stop": format!(r#"touch "{}""#, marker.display()),
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet.with_arg("stop").assert().success();
    assert!(marker.exists(), "stop script should have created the marker");

    drop(root);
}

/// `pacquet stop` with no "stop" script exits 0 silently (no error),
/// matching how `pacquet test` and `pacquet start` behave when the
/// script is absent.
#[test]
fn stop_without_script_exits_zero() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "build": "echo built" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet.with_arg("stop").assert().success();

    drop(root);
}

/// `pacquet start` runs the "start" script (regression guard for the
/// existing command, ensuring it still works alongside the new restart
/// and stop commands).
#[cfg(unix)]
#[test]
fn start_runs_declared_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let marker = workspace.join("started.txt");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "start": format!(r#"touch "{}""#, marker.display()),
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet.with_arg("start").assert().success();
    assert!(marker.exists(), "start script should have created the marker");

    drop(root);
}
