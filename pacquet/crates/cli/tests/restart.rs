use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::fs;

/// `pacquet restart` runs "stop", "restart", and "start" scripts
/// sequentially. Each script appends to a log file; their order
/// in the file proves execution order.
#[cfg(unix)]
#[test]
fn restart_runs_stop_restart_start_scripts() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let log_file = workspace.join("log.txt");
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "stop": format!(r#"echo stop >> "{}""#, log_file.display()),
            "restart": format!(r#"echo restart >> "{}""#, log_file.display()),
            "start": format!(r#"echo start >> "{}""#, log_file.display()),
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet.with_arg("restart").assert().success();
    let content = fs::read_to_string(&log_file).expect("read log file");
    let lines: Vec<&str> = content.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, vec!["stop", "restart", "start"]);

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
    let log_file = workspace.join("log.txt");
    let append_arg_node = |name: &str| {
        format!(
            "node -e \"require('fs').appendFileSync('{}', '{} ' + process.argv[1] + '\\n')\"",
            log_file.display(),
            name,
        )
    };
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "stop": append_arg_node("stop"),
            "restart": append_arg_node("restart"),
            "start": append_arg_node("start"),
        },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet.with_arg("restart").with_arg("hello-world").assert().success();
    let content = fs::read_to_string(&log_file).expect("read log file");
    let lines: Vec<&str> = content.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, vec!["stop hello-world", "restart hello-world", "start hello-world"]);

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

/// `pacquet stop` with no "stop" script fails (exit code 1) when --if-present is absent.
#[test]
fn stop_without_script_fails() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "build": "echo built" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet.with_arg("stop").assert().failure();

    drop(root);
}

/// `pacquet stop` with no "stop" script succeeds with --if-present.
#[test]
fn stop_without_script_succeeds_with_if_present() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "build": "echo built" },
    })
    .to_string();
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");

    pacquet.with_arg("stop").with_arg("--if-present").assert().success();

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
