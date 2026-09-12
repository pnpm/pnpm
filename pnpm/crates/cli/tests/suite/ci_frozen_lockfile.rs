use crate::_utils::pacquet_in;

use assert_cmd::prelude::*;
use std::{fs, path::Path, process::Command};
use tempfile::{TempDir, tempdir};

const OUTDATED_LOCKFILE: &str = "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      old-dependency:\n        specifier: file:dependency\n        version: link:dependency\n";
const EMPTY_LOCKFILE: &str = "lockfileVersion: '9.0'\nimporters:\n  .: {}\n";

fn pacquet_in_ci(workspace: &Path) -> Command {
    let mut command = pacquet_without_ci(workspace);
    command.env("CI", "true");
    command
}

fn pacquet_in_github_actions(workspace: &Path) -> Command {
    let mut command = pacquet_without_ci(workspace);
    command.env("GITHUB_ACTIONS", "true");
    command
}

fn pacquet_without_ci(workspace: &Path) -> Command {
    let mut command = pacquet_in(workspace);
    command
        .env_remove("PNPM_CONFIG_CI")
        .env_remove("CI")
        .env_remove("GITHUB_ACTION")
        .env_remove("GITHUB_ACTIONS");
    command
}

fn outdated_lockfile_project() -> TempDir {
    let root = tempdir().expect("create temp directory");
    let workspace = root.path();
    let dependency_dir = workspace.join("dependency");
    fs::create_dir(&dependency_dir).expect("create local dependency directory");
    fs::write(
        dependency_dir.join("package.json"),
        serde_json::json!({ "name": "local-dependency", "version": "1.0.0" }).to_string(),
    )
    .expect("write local dependency manifest");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "local-dependency": "file:dependency" } })
            .to_string(),
    )
    .expect("write project manifest");
    fs::write(workspace.join("pnpm-lock.yaml"), OUTDATED_LOCKFILE).expect("write stale lockfile");
    root
}

fn assert_lockfile_was_updated(workspace: &Path) {
    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile after install");
    assert_ne!(lockfile, OUTDATED_LOCKFILE);
    assert!(
        lockfile.contains("local-dependency"),
        "updated lockfile must contain the local dependency; got:\n{lockfile}",
    );
}

#[test]
fn ci_rejects_an_outdated_lockfile_by_default() {
    for command_in_ci in [pacquet_in_ci, pacquet_in_github_actions] {
        let root = outdated_lockfile_project();
        let workspace = root.path();
        let lockfile_path = workspace.join("pnpm-lock.yaml");

        let assert = command_in_ci(workspace).arg("install").assert().failure();
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
        eprintln!("STDERR:\n{stderr}\n");
        assert!(
            stderr.contains("ERR_PNPM_OUTDATED_LOCKFILE"),
            "CI install must report the outdated lockfile; got:\n{stderr}",
        );
        assert_eq!(
            fs::read_to_string(lockfile_path).expect("read lockfile after failed install"),
            OUTDATED_LOCKFILE,
        );
    }

    let root = outdated_lockfile_project();
    let workspace = root.path();
    pacquet_in(workspace)
        .env("CI", "true")
        .env("PNPM_CONFIG_CI", "false")
        .arg("install")
        .assert()
        .success();
}

#[test]
fn ci_values_enable_the_default_in_github_actions() {
    for ci_value in ["1", "true"] {
        let root = outdated_lockfile_project();
        let workspace = root.path();

        let assert = pacquet_without_ci(workspace)
            .env("CI", ci_value)
            .env("GITHUB_ACTIONS", "true")
            .arg("install")
            .assert()
            .failure();
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
        eprintln!("STDERR:\n{stderr}\n");
        assert!(
            stderr.contains("ERR_PNPM_OUTDATED_LOCKFILE"),
            "CI={ci_value} in GitHub Actions must report the outdated lockfile; got:\n{stderr}",
        );
    }
}

#[test]
fn ci_false_disables_the_default_in_github_actions() {
    let root = outdated_lockfile_project();
    let workspace = root.path();

    pacquet_without_ci(workspace)
        .env("CI", "false")
        .env("GITHUB_ACTIONS", "true")
        .arg("install")
        .assert()
        .success();

    assert_lockfile_was_updated(workspace);
}

#[test]
fn workspace_manifest_cannot_disable_ci_detection() {
    let root = outdated_lockfile_project();
    let workspace = root.path();
    fs::write(workspace.join("pnpm-workspace.yaml"), "ci: false\n")
        .expect("write workspace manifest");

    let assert = pacquet_in_ci(workspace).arg("install").assert().failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("ERR_PNPM_OUTDATED_LOCKFILE"),
        "workspace ci:false must not disable CI detection; got:\n{stderr}",
    );
    assert_eq!(
        fs::read_to_string(workspace.join("pnpm-lock.yaml"))
            .expect("read lockfile after failed install"),
        OUTDATED_LOCKFILE,
    );
}

#[test]
fn ci_honors_explicit_prefer_frozen_lockfile_values() {
    for prefer_arg in ["--prefer-frozen-lockfile", "--no-prefer-frozen-lockfile"] {
        let root = outdated_lockfile_project();
        let workspace = root.path();

        pacquet_in_ci(workspace).args(["install", prefer_arg]).assert().success();

        assert_lockfile_was_updated(workspace);
    }
}

#[test]
fn ci_honors_configured_prefer_frozen_lockfile_values() {
    for prefer_value in ["true", "false"] {
        let root = outdated_lockfile_project();
        let workspace = root.path();

        pacquet_in_ci(workspace)
            .env("PNPM_CONFIG_PREFER_FROZEN_LOCKFILE", prefer_value)
            .arg("install")
            .assert()
            .success();

        assert_lockfile_was_updated(workspace);
    }
}

#[test]
fn ci_honors_pnpmfile_prefer_frozen_lockfile_values() {
    for prefer_value in [true, false] {
        let root = outdated_lockfile_project();
        let workspace = root.path();
        fs::write(
            workspace.join(".pnpmfile.cjs"),
            format!(
                "module.exports = {{ hooks: {{ updateConfig (config) {{ config.preferFrozenLockfile = {prefer_value}; return config }} }} }}",
            ),
        )
        .expect("write updateConfig hook");

        pacquet_in_ci(workspace).arg("install").assert().success();

        assert_lockfile_was_updated(workspace);
    }
}

#[test]
fn explicit_frozen_lockfile_values_take_priority_over_prefer_flags() {
    for prefer_arg in ["--prefer-frozen-lockfile", "--no-prefer-frozen-lockfile"] {
        let frozen_root = outdated_lockfile_project();
        let frozen_workspace = frozen_root.path();
        let assert = pacquet_in_ci(frozen_workspace)
            .args(["install", prefer_arg, "--frozen-lockfile"])
            .assert()
            .failure();
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
        assert!(stderr.contains("ERR_PNPM_OUTDATED_LOCKFILE"), "got:\n{stderr}");
        assert_eq!(
            fs::read_to_string(frozen_workspace.join("pnpm-lock.yaml"))
                .expect("read lockfile after failed install"),
            OUTDATED_LOCKFILE,
        );

        let mutable_root = outdated_lockfile_project();
        let mutable_workspace = mutable_root.path();
        pacquet_in_ci(mutable_workspace)
            .args(["install", prefer_arg, "--no-frozen-lockfile"])
            .assert()
            .success();
        assert_lockfile_was_updated(mutable_workspace);
    }
}

#[test]
fn configured_frozen_lockfile_values_take_priority_over_prefer_flags() {
    for prefer_arg in ["--prefer-frozen-lockfile", "--no-prefer-frozen-lockfile"] {
        let frozen_root = outdated_lockfile_project();
        let frozen_workspace = frozen_root.path();
        let assert = pacquet_in_ci(frozen_workspace)
            .env("PNPM_CONFIG_FROZEN_LOCKFILE", "true")
            .args(["install", prefer_arg])
            .assert()
            .failure();
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
        assert!(stderr.contains("ERR_PNPM_OUTDATED_LOCKFILE"), "got:\n{stderr}");
        assert_eq!(
            fs::read_to_string(frozen_workspace.join("pnpm-lock.yaml"))
                .expect("read lockfile after failed install"),
            OUTDATED_LOCKFILE,
        );

        let mutable_root = outdated_lockfile_project();
        let mutable_workspace = mutable_root.path();
        pacquet_in_ci(mutable_workspace)
            .env("PNPM_CONFIG_FROZEN_LOCKFILE", "false")
            .args(["install", prefer_arg])
            .assert()
            .success();
        assert_lockfile_was_updated(mutable_workspace);
    }
}

#[test]
fn ci_install_without_a_nonempty_lockfile_generates_one() {
    for create_empty_lockfile in [false, true] {
        let root = tempdir().expect("create temp directory");
        let workspace = root.path();
        fs::write(workspace.join("package.json"), "{}").expect("write project manifest");
        if create_empty_lockfile {
            fs::write(workspace.join("pnpm-lock.yaml"), "").expect("write empty lockfile");
        }

        pacquet_in_ci(workspace).arg("install").assert().success();

        let lockfile =
            fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read generated lockfile");
        assert!(!lockfile.is_empty(), "CI install must create a non-empty lockfile");
    }
}

#[test]
fn ci_install_with_a_semantically_empty_lockfile_updates_it() {
    let root = outdated_lockfile_project();
    let workspace = root.path();
    fs::write(workspace.join("pnpm-lock.yaml"), EMPTY_LOCKFILE)
        .expect("write semantically empty lockfile");

    pacquet_in_ci(workspace).arg("install").assert().success();

    assert_lockfile_was_updated(workspace);
}
