//! `pacquet doctor` end to end: the report on a fresh environment and on a
//! blocked store, and the lifecycle script whose `.bin` shim has to find
//! `node`, both in a temporary project and in the project it is run from.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde::Deserialize;
use serde_json::Value;
use std::{
    ffi::OsString,
    fs,
    path::Path,
    process::Command,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    checks: Vec<Check>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Check {
    title: String,
    status: String,
    detail: Option<String>,
}

fn prepend_to_path(dir: &Path) -> OsString {
    let mut entries = vec![dir.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        entries.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(entries).expect("join PATH")
}

#[test]
fn succeeds_on_a_fresh_environment_before_cache_or_store_dirs_exist() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let home = temp.path().join("home");
    let project = temp.path().join("proj");
    let cache_home = temp.path().join("cache");
    let data_home = temp.path().join("data");
    let state_home = temp.path().join("state");
    let config_home = temp.path().join("config");
    let pnpm_home = temp.path().join("pnpm-home");
    let auth_file = temp.path().join("empty-npmrc");

    fs::create_dir_all(&home).expect("create home");
    fs::create_dir_all(&project).expect("create project");
    fs::write(project.join("package.json"), r#"{"name":"demo-doctor","version":"1.0.0"}"#)
        .expect("write package.json");
    fs::write(&auth_file, "").expect("write empty auth file");

    let output = Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(&project)
        .with_env("HOME", &home)
        .with_env("USERPROFILE", &home)
        .with_env("LOCALAPPDATA", &data_home)
        .with_env("APPDATA", &config_home)
        .with_env("XDG_CACHE_HOME", &cache_home)
        .with_env("XDG_DATA_HOME", &data_home)
        .with_env("XDG_STATE_HOME", &state_home)
        .with_env("XDG_CONFIG_HOME", &config_home)
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("PATH", prepend_to_path(&pnpm_home))
        .without_env("COREPACK_ROOT")
        .without_env("PNPM_CONFIG_STORE_DIR")
        .without_env("PNPM_CONFIG_CACHE_DIR")
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("--dir")
        .with_arg(&project)
        .with_arg("doctor")
        .with_arg("--offline")
        .with_arg("--json")
        .output()
        .expect("run pnpm doctor");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "doctor must succeed on a fresh environment\nstdout:\n{stdout}\nstderr:\n{stderr}",
    );

    let report: Report = serde_json::from_str(&stdout).expect("parse doctor JSON output");
    for expected in [
        "Global bin directory",
        "Cache directory",
        "Store directory",
        "Filesystem",
        "Registry connectivity",
        "Install smoke test",
    ] {
        let check = report.checks
            .iter()
            .find(|candidate| candidate.title == expected)
            .unwrap_or_else(|| panic!("missing check {expected:?} in {report:?}"));
        assert_eq!(check.status, "pass", "check {expected:?} must pass: {check:?}");
    }

    assert!(!cache_home.exists(), "doctor must not create XDG_CACHE_HOME");
    assert!(!pnpm_home.join("store").exists(), "doctor must not create the store directory");
    assert!(!project.join("node_modules").exists(), "doctor must not create node_modules");
}

#[test]
fn fails_when_store_directory_is_blocked_by_a_regular_file() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = temp.path().join("proj");
    let blocker = temp.path().join("blocker-file");
    let auth_file = temp.path().join("empty-npmrc");

    fs::create_dir_all(&project).expect("create project");
    fs::write(project.join("package.json"), r#"{"name":"demo-doctor","version":"1.0.0"}"#)
        .expect("write package.json");
    fs::write(&blocker, "not a directory").expect("write blocker file");
    fs::write(&auth_file, "").expect("write empty auth file");

    let blocked_store = blocker.join("store");
    let output = Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(&project)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("--dir")
        .with_arg(&project)
        .with_arg(format!("--store-dir={}", blocked_store.display()))
        .with_arg("doctor")
        .with_arg("--offline")
        .with_arg("--json")
        .output()
        .expect("run pnpm doctor");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !output.status.success(),
        "doctor must fail when store-dir cannot be created\nstdout:\n{stdout}",
    );

    let report: Report = serde_json::from_str(&stdout).expect("parse doctor JSON output");
    let store_check = report.checks
        .iter()
        .find(|candidate| candidate.title == "Store directory")
        .expect("Store directory check");
    assert_eq!(store_check.status, "fail");
    assert!(
        store_check.detail
            .as_deref()
            .is_some_and(|detail| detail.starts_with("no write access to ")),
        "unexpected Store directory detail: {store_check:?}",
    );

    let fs_check = report.checks
        .iter()
        .find(|candidate| candidate.title == "Filesystem")
        .expect("Filesystem check");
    assert_eq!(
        fs_check.status, "warn",
        "Filesystem check must warn rather than falling back to the system temp dir: {fs_check:?}",
    );
}

#[test]
fn install_method_warns_about_a_pnpm_ahead_of_the_global_bin() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let home = temp.path().join("home");
    let project = temp.path().join("proj");
    let pnpm_home = temp.path().join("pnpm-home");
    let other = temp.path().join("other-bin");
    let auth_file = temp.path().join("empty-npmrc");

    fs::create_dir_all(&home).expect("create home");
    fs::create_dir_all(&project).expect("create project");
    fs::create_dir_all(pnpm_home.join("bin")).expect("create global bin");
    fs::create_dir_all(&other).expect("create other bin");
    fs::write(project.join("package.json"), r#"{"name":"demo-doctor","version":"1.0.0"}"#)
        .expect("write package.json");
    fs::write(&auth_file, "").expect("write empty auth file");
    let other_pnpm = other.join(if cfg!(windows) { "pnpm.cmd" } else { "pnpm" });
    fs::write(&other_pnpm, "").expect("write the other pnpm");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&other_pnpm, fs::Permissions::from_mode(0o755))
            .expect("make the other pnpm executable");
    }
    let path = std::env::join_paths([&other, &pnpm_home.join("bin")]).expect("join PATH");

    let output = Command::cargo_bin("pnpm")
        .expect("find pnpm binary")
        .with_current_dir(&project)
        .with_env("HOME", &home)
        .with_env("USERPROFILE", &home)
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("PATH", path)
        .without_env("COREPACK_ROOT")
        .without_env("PNPM_CONFIG_GLOBAL_BIN_DIR")
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_arg("--dir")
        .with_arg(&project)
        .with_arg("doctor")
        .with_arg("--offline")
        .with_arg("--json")
        .output()
        .expect("run pnpm doctor");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let report: Report = serde_json::from_str(&stdout).expect("parse doctor JSON output");
    let check = report.checks
        .iter()
        .find(|candidate| candidate.title == "Install method")
        .expect("Install method check");
    assert_eq!(check.status, "warn", "{check:?}");
    let detail = check.detail.as_deref().unwrap_or_default();
    assert!(detail.contains(&other_pnpm.display().to_string()), "{check:?}");
}

fn doctor(pacquet: Command, root: &Path) -> Value {
    let output = pacquet
        .with_arg(format!("--config.store-dir={}", root.join("store").display()))
        .with_arg(format!("--config.cache-dir={}", root.join("cache").display()))
        .with_args(["doctor", "--offline", "--json"])
        .output()
        .expect("run pacquet doctor");
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("STDOUT:\n{stdout}\nSTDERR:\n{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_str(&stdout).expect("parse the doctor report")
}

fn find_check<'a>(report: &'a Value, title: &str) -> &'a Value {
    report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .find(|check| check["title"] == title)
        .unwrap_or_else(|| panic!("no {title:?} check in {report:#}"))
}

#[test]
fn runs_a_package_executable_from_a_script_in_the_current_project() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"doctor-project","version":"1.0.0"}"#)
        .expect("write package.json");

    let report = doctor(pacquet, root.path());

    let scripts = find_check(&report, "Lifecycle scripts");
    dbg!(scripts);
    assert_eq!(scripts["status"], "pass");
    let detail = scripts["detail"].as_str().expect("detail");
    assert!(detail.starts_with("an install script ran "), "{detail}");
    // The project path is spelled as the child sees it: `/private/var` on
    // macOS and the long user name on Windows, so match its last segment.
    assert!(detail.contains(", a script in "), "{detail}");
    assert!(detail.contains("workspace ran "), "{detail}");
    assert!(
        !workspace.join("node_modules").exists(),
        "the probe must not install into the project",
    );
}

/// The probe executable's shim is looked up in the temporary project's
/// `node_modules`, whatever modules directory the environment configures.
#[test]
fn the_probe_install_ignores_a_configured_modules_dir() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"doctor-project","version":"1.0.0"}"#)
        .expect("write package.json");

    let report = doctor(pacquet.with_env("PNPM_CONFIG_MODULES_DIR", "vendor"), root.path());

    let scripts = find_check(&report, "Lifecycle scripts");
    dbg!(scripts);
    assert_eq!(scripts["status"], "pass");
}

/// A setting given to `pnpm doctor` on the command line reaches the run in the
/// current project, as it would reach `pnpm exec` there.
#[test]
fn the_project_probe_gets_the_command_line_settings() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"doctor-project","version":"1.0.0"}"#)
        .expect("write package.json");
    let missing = root.path().join("missing-preload.js");

    let report = doctor(
        pacquet.with_arg(format!("--config.node-options=--require={}", missing.display())),
        root.path(),
    );

    let scripts = find_check(&report, "Lifecycle scripts");
    dbg!(scripts);
    assert_eq!(scripts["status"], "fail");
    let detail = scripts["detail"].as_str().expect("detail");
    assert!(detail.starts_with("a script in "), "{detail}");
}

/// `verifyDepsBeforeRun` set in the environment outranks the command line, so
/// the probe has to override it there to keep from installing the project.
#[test]
fn the_project_probe_does_not_install_when_the_environment_asks_to() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let dep = workspace.join("dep");
    fs::create_dir_all(&dep).expect("create dep dir");
    fs::write(dep.join("package.json"), r#"{"name":"dep","version":"1.0.0"}"#)
        .expect("write dep package.json");
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"doctor-project","version":"1.0.0","dependencies":{"dep":"file:./dep"}}"#,
    )
    .expect("write package.json");

    let report =
        doctor(pacquet.with_env("PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN", "install"), root.path());

    let scripts = find_check(&report, "Lifecycle scripts");
    dbg!(scripts);
    assert_eq!(scripts["status"], "pass");
    assert!(
        !workspace.join("node_modules").exists(),
        "the probe must not install into the project",
    );
}

/// A `node` that `pnpm` sees but that cannot run a script is what a script
/// fails on with exit status 127; the report has to carry the shim's trace.
#[cfg(unix)]
#[test]
fn a_failing_script_is_reported_with_the_shim_trace() {
    use std::os::unix::fs::PermissionsExt;

    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let fake_bin = root.path().join("fake-bin");
    fs::create_dir_all(&fake_bin).expect("create fake bin dir");
    let fake_node = fake_bin.join("node");
    fs::write(&fake_node, "#!/bin/sh\nexit 127\n").expect("write fake node");
    fs::set_permissions(&fake_node, fs::Permissions::from_mode(0o755)).expect("chmod fake node");
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());

    let report = doctor(pacquet.with_env("PATH", path), root.path());

    let node = find_check(&report, "Node.js on PATH");
    dbg!(node);
    let node_detail = node["detail"].as_str().expect("detail");
    assert!(node_detail.starts_with(&fake_node.display().to_string()), "{node_detail}");
    let scripts = find_check(&report, "Lifecycle scripts");
    dbg!(scripts);
    assert_eq!(scripts["status"], "fail");
    let detail = scripts["detail"].as_str().expect("detail");
    assert!(detail.starts_with("the install script failed: "), "{detail}");
    assert!(detail.contains("exited with exit status: 127"), "{detail}");
    assert!(detail.contains("+ exec node "), "the trace must show the shim's exec: {detail}");
}
