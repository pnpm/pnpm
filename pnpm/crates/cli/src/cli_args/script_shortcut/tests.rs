use super::ScriptShortcutArgs;
use crate::cli_args::reporter::ReporterType;
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

#[cfg_attr(windows, ignore = "runs a `touch` shell command, which Windows does not provide")]
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
    ScriptShortcutArgs { args: vec![] }
        .into_run_args("stop")
        .run(dir, &config, ReporterType::Silent)
        .expect("stop should succeed");
    assert!(marker.exists(), "stop script should have run");
}

#[test]
fn stop_with_if_present_skips_missing_script() {
    let tmp = TempDir::new().expect("tmp dir");
    let dir = tmp.path();
    setup_project(dir, &json!({}));
    let config = test_config();
    let mut args = ScriptShortcutArgs { args: vec![] }.into_run_args("stop");
    args.if_present = true;
    args.run(dir, &config, ReporterType::Silent)
        .expect("--if-present should succeed when script is missing");
}

#[test]
fn stop_fails_on_missing_script_without_if_present() {
    let tmp = TempDir::new().expect("tmp dir");
    let dir = tmp.path();
    setup_project(dir, &json!({}));
    let config = test_config();
    let res = ScriptShortcutArgs { args: vec![] }
        .into_run_args("stop")
        .run(dir, &config, ReporterType::Silent);
    assert!(res.is_err(), "should fail because script is missing");
}

#[test]
fn test_fails_on_missing_script_without_if_present() {
    let tmp = TempDir::new().expect("tmp dir");
    let dir = tmp.path();
    setup_project(dir, &json!({}));
    let config = test_config();
    let res = ScriptShortcutArgs { args: vec![] }
        .into_run_args("test")
        .run(dir, &config, ReporterType::Silent);
    assert!(res.is_err(), "test shortcut should fail when scripts.test is missing");
}

#[test]
fn start_with_if_present_skips_when_script_and_server_js_are_missing() {
    let tmp = TempDir::new().expect("tmp dir");
    let dir = tmp.path();
    setup_project(dir, &json!({}));
    let config = test_config();
    let mut args = ScriptShortcutArgs { args: vec![] }.into_run_args("start");
    args.if_present = true;
    args.run(dir, &config, ReporterType::Silent)
        .expect(
            "--if-present start should succeed when neither scripts.start nor server.js exists",
        );
}

/// These tests drive `RunArgs::run` in-process, so the
/// verify-deps-before-run gate must stay off: its `install` default
/// would spawn `current_exe()` — the test harness binary — as the
/// installer. pnpm's unit tests equally construct their options
/// without the setting.
fn test_config() -> pnpm_config::Config {
    pnpm_config::Config {
        verify_deps_before_run: pnpm_config::VerifyDepsBeforeRun::False,
        ..pnpm_config::Config::default()
    }
}
