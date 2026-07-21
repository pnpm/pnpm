use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use serde_json::Value;
use std::{fs, process::Command};

fn write_project(workspace: &std::path::Path, relative_dir: &str, name: &str) {
    let project_dir = workspace.join(relative_dir);
    fs::create_dir_all(&project_dir).expect("create project directory");
    fs::write(
        project_dir.join("package.json"),
        serde_json::json!({ "name": name, "version": "1.0.0" }).to_string(),
    )
    .expect("write project manifest");
}

fn run_peers(workspace: &std::path::Path, args: &[&str]) -> Value {
    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_args(args)
        .output()
        .expect("run pnpm peers");
    assert!(output.status.success(), "peers should succeed: {output:?}");
    serde_json::from_slice(&output.stdout).expect("parse peers JSON")
}

#[test]
fn peers_is_recursive_by_default_and_honors_filters() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    fs::write(workspace.join("package.json"), r#"{ "name": "root", "version": "1.0.0" }"#)
        .expect("write root manifest");
    write_project(&workspace, "packages/app-a", "app-a");
    write_project(&workspace, "packages/app-b", "app-b");
    fs::write(
        workspace.join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\nimporters:\n  .: {}\n  packages/app-a: {}\n  packages/app-b: {}\n",
    )
    .expect("write lockfile");

    let all = run_peers(&workspace, &["peers", "--lockfile-only", "--json"]);
    assert_eq!(all.as_object().map(serde_json::Map::len), Some(3));

    let filtered =
        run_peers(&workspace, &["--filter", "app-a", "peers", "--lockfile-only", "--json"]);
    let filtered = filtered.as_object().expect("filtered peer issues object");
    assert_eq!(filtered.len(), 1);
    assert!(filtered.contains_key("packages/app-a"));

    drop(root);
}

#[test]
fn recursive_peers_uses_the_active_dedicated_lockfile() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write workspace manifest");
    fs::write(workspace.join("package.json"), r#"{ "name": "root", "version": "1.0.0" }"#)
        .expect("write root manifest");
    write_project(&workspace, "packages/app", "app");
    let app = workspace.join("packages/app");
    fs::write(app.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\nimporters:\n  .: {}\n")
        .expect("write dedicated lockfile");

    let issues = run_peers(&app, &["peers", "--lockfile-only", "--json"]);
    let issues = issues.as_object().expect("peer issues object");
    assert_eq!(issues.len(), 1);
    assert!(issues.contains_key("."));

    drop(root);
}
