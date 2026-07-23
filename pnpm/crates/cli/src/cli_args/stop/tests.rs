// Every test here drives `/bin/sh`-style lifecycle scripts, so the whole
// module is Unix-only; gating it keeps Windows builds free of unused-import
// and dead-code warnings.
#![cfg(unix)]

use super::StopArgs;
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
fn stop_runs_declared_script() {
    let tmp = TempDir::new().expect("tmp dir");
    let dir = tmp.path();
    let marker = dir.join("stopped.txt");
    setup_project(
        dir,
        &json!({
            "stop": format!(r#"touch "{}""#, marker.display()),
        }),
    );
    let config = test_config();
    StopArgs { args: vec![], if_present: false }
        .run(dir, &config, true)
        .expect("stop should succeed");
    assert!(marker.exists(), "stop script should have run");
}

#[cfg(unix)]
#[test]
fn stop_with_if_present_skips_missing_script() {
    let tmp = TempDir::new().expect("tmp dir");
    let dir = tmp.path();
    setup_project(dir, &json!({}));
    let config = test_config();
    StopArgs { args: vec![], if_present: true }
        .run(dir, &config, true)
        .expect("--if-present should succeed when script is missing");
}

#[cfg(unix)]
#[test]
fn stop_fails_on_missing_script_without_if_present() {
    let tmp = TempDir::new().expect("tmp dir");
    let dir = tmp.path();
    setup_project(dir, &json!({}));
    let config = test_config();
    let res = StopArgs { args: vec![], if_present: false }.run(dir, &config, true);
    assert!(res.is_err(), "should fail because script is missing");
}

/// These tests drive `StopArgs::run` in-process, so the
/// verify-deps-before-run gate must stay off: its `install` default
/// would spawn `current_exe()` — the test harness binary — as the
/// installer. pnpm's unit tests equally construct their options
/// without the setting.
fn test_config() -> pacquet_config::Config {
    pacquet_config::Config {
        verify_deps_before_run: pacquet_config::VerifyDepsBeforeRun::False,
        ..pacquet_config::Config::default()
    }
}
