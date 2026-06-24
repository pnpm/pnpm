// Every test here drives `/bin/sh`-style lifecycle scripts, so the whole
// module is Unix-only; gating it keeps Windows builds free of unused-import
// and dead-code warnings.
#![cfg(unix)]

use super::RestartArgs;
use serde_json::json;
use tempfile::TempDir;

fn setup_project(dir: &std::path::Path, scripts: &serde_json::Value) {
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": scripts,
    });
    std::fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
}

#[cfg(unix)]
#[test]
fn restart_runs_stop_restart_start_in_order() {
    let tmp = TempDir::new().expect("tmp dir");
    let dir = tmp.path();
    let log_file = dir.join("log.txt");
    setup_project(
        dir,
        &json!({
            "stop": format!("echo stop >> \"{}\"", log_file.display()),
            "restart": format!("echo restart >> \"{}\"", log_file.display()),
            "start": format!("echo start >> \"{}\"", log_file.display()),
        }),
    );
    let config = pacquet_config::Config::default();
    RestartArgs { args: vec![], if_present: false }
        .run(dir, &config, true)
        .expect("restart should succeed");
    let content = std::fs::read_to_string(&log_file).expect("read log file");
    let lines: Vec<&str> = content.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, vec!["stop", "restart", "start"]);
}

#[cfg(unix)]
#[test]
fn restart_with_if_present_skips_missing_stop_and_restart() {
    let tmp = TempDir::new().expect("tmp dir");
    let dir = tmp.path();
    setup_project(
        dir,
        &json!({
            "start": "exit 0",
        }),
    );
    let config = pacquet_config::Config::default();
    RestartArgs { args: vec![], if_present: true }
        .run(dir, &config, true)
        .expect("--if-present should skip missing stop/restart");
}

#[cfg(unix)]
#[test]
fn restart_passes_args_to_each_script() {
    let tmp = TempDir::new().expect("tmp dir");
    let dir = tmp.path();
    let log_file = dir.join("log.txt");
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
    });
    std::fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    let config = pacquet_config::Config::default();
    RestartArgs { args: vec!["myarg".to_string()], if_present: false }
        .run(dir, &config, true)
        .expect("restart should succeed");
    let content = std::fs::read_to_string(&log_file).expect("read log file");
    let lines: Vec<&str> = content.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    assert_eq!(lines, vec!["stop myarg", "restart myarg", "start myarg"]);
}
