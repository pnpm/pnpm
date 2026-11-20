use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use serde::Deserialize;
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
