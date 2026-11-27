use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils_registry::bin::CommandTempCwd;
use std::{
    fs,
    path::Path,
    process::Command,
};

fn write_project(workspace: &Path, relative_dir: &str, name: &str) {
    let project_dir = workspace.join(relative_dir);
    fs::create_dir_all(&project_dir).expect("create project directory");
    fs::write(
        project_dir.join("package.json"),
        serde_json::json!({
            "name": name,
            "version": "1.0.0",
            "scripts": {
                "install": r#"node -e "require('fs').writeFileSync('rebuilt.txt','ran')""#,
            },
        })
        .to_string(),
    )
    .expect("write project manifest");
}

fn filtered_rebuild_only_runs_selected_project(shared_workspace_lockfile: bool) {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let dedicated_setting =
        if shared_workspace_lockfile { "" } else { "sharedWorkspaceLockfile: false\n" };
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("packages:\n  - packages/*\n{dedicated_setting}"),
    )
    .expect("write workspace manifest");
    fs::write(workspace.join("package.json"), r#"{ "name": "root", "version": "1.0.0" }"#)
        .expect("write root manifest");
    write_project(&workspace, "packages/app-a", "app-a");
    write_project(&workspace, "packages/app-b", "app-b");

    pacquet
        .with_args(["install", "--ignore-scripts", "--reporter=silent"])
        .assert()
        .success();
    let selected_marker = workspace.join("packages/app-a/rebuilt.txt");
    let unselected_marker = workspace.join("packages/app-b/rebuilt.txt");
    assert!(!selected_marker.exists());
    assert!(!unselected_marker.exists());

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["--filter", "app-a", "rebuild", "--pending", "--reporter=silent"])
        .assert()
        .success();

    assert!(selected_marker.exists(), "selected project install script should run");
    assert!(!unselected_marker.exists(), "unselected project install script should not run");

    drop(root);
}

#[test]
fn filtered_rebuild_selects_projects_with_a_shared_lockfile() {
    filtered_rebuild_only_runs_selected_project(true);
}

#[test]
fn filtered_rebuild_selects_projects_with_dedicated_lockfiles() {
    filtered_rebuild_only_runs_selected_project(false);
}

#[test]
fn dedicated_recursive_rebuild_no_bail_runs_dependents_after_failures() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write workspace manifest");
    fs::write(workspace.join("package.json"), r#"{ "name": "root", "version": "1.0.0" }"#)
        .expect("write root manifest");
    let app_a = workspace.join("packages/app-a");
    let app_b = workspace.join("packages/app-b");
    fs::create_dir_all(&app_a).expect("create app-a");
    fs::create_dir_all(&app_b).expect("create app-b");
    fs::write(
        app_a.join("package.json"),
        serde_json::json!({
            "name": "app-a",
            "version": "1.0.0",
            "scripts": { "install": r#"node -e "process.exit(1)""# },
        })
        .to_string(),
    )
    .expect("write app-a manifest");
    fs::write(
        app_b.join("package.json"),
        serde_json::json!({
            "name": "app-b",
            "version": "1.0.0",
            "dependencies": { "app-a": "workspace:*" },
            "scripts": {
                "install": r#"node -e "require('fs').writeFileSync('rebuilt.txt','ran')""#,
            },
        })
        .to_string(),
    )
    .expect("write app-b manifest");

    pacquet
        .with_args(["install", "--ignore-scripts", "--reporter=silent"])
        .assert()
        .success();
    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["-r", "--no-bail", "rebuild", "--pending", "--reporter=silent"])
        .output()
        .expect("run recursive rebuild");

    assert!(!output.status.success(), "the failed project should fail the command");
    assert!(
        app_b.join("rebuilt.txt").exists(),
        "--no-bail should continue to the dependent project after its dependency fails: {output:?}",
    );

    drop(root);
}

fn write_directory_dep(parent: &Path, name: &str, marker: &str) {
    let dir = parent.join(name);
    fs::create_dir_all(&dir).expect("create directory dependency");
    fs::write(
        dir.join("package.json"),
        serde_json::json!({
            "name": name,
            "version": "1.0.0",
            "scripts": {
                "postinstall": "node build.cjs",
            },
        })
        .to_string(),
    )
    .expect("write directory dependency manifest");
    fs::write(
        dir.join("build.cjs"),
        format!("require('node:fs').writeFileSync('built.txt', {marker:?})\n"),
    )
    .expect("write directory dependency build script");
}

fn assert_selected_rebuild_keeps_unselected_directory_dep(node_linker: &str) {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_directory_dep(root.path(), "dep1", "v1");
    write_directory_dep(root.path(), "dep2", "v1");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "proj",
            "version": "1.0.0",
            "dependencies": {
                "dep1": "file:../dep1",
                "dep2": "file:../dep2",
            },
        })
        .to_string(),
    )
    .expect("write project manifest");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "nodeLinker: {node_linker}\nallowBuilds:\n  \"dep1@file:../dep1\": true\n  \"dep2@file:../dep2\": true\n",
        ),
    )
    .expect("write workspace settings");

    pacquet
        .with_args(["install", "--reporter=silent"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(workspace.join("node_modules/dep1/built.txt")).ok().as_deref(),
        Some("v1"),
        "initial install builds dep1",
    );
    assert_eq!(
        fs::read_to_string(workspace.join("node_modules/dep2/built.txt")).ok().as_deref(),
        Some("v1"),
        "initial install builds dep2",
    );

    write_directory_dep(root.path(), "dep1", "v2");

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["rebuild", "dep1", "--reporter=silent"])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(workspace.join("node_modules/dep1/built.txt")).ok().as_deref(),
        Some("v2"),
        "selected directory dependency is re-imported and rebuilt",
    );
    assert_eq!(
        fs::read_to_string(workspace.join("node_modules/dep2/built.txt")).ok().as_deref(),
        Some("v1"),
        "unselected directory dependency must keep its build output",
    );

    drop(root);
}

#[test]
fn selected_rebuild_keeps_unselected_directory_dep_build_output_isolated() {
    assert_selected_rebuild_keeps_unselected_directory_dep("isolated");
}

#[test]
fn selected_rebuild_keeps_unselected_directory_dep_build_output_hoisted() {
    assert_selected_rebuild_keeps_unselected_directory_dep("hoisted");
}

#[test]
fn approve_builds_keeps_unselected_directory_dep_build_output() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_directory_dep(root.path(), "dep1", "v1");
    write_directory_dep(root.path(), "dep2", "v1");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "proj",
            "version": "1.0.0",
            "dependencies": {
                "dep1": "file:../dep1",
                "dep2": "file:../dep2",
            },
        })
        .to_string(),
    )
    .expect("write project manifest");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "strictDepBuilds: false\nallowBuilds:\n  \"dep2@file:../dep2\": true\n",
    )
    .expect("write workspace settings");

    pacquet
        .with_args(["install", "--reporter=silent"])
        .assert()
        .success();
    assert!(!workspace.join("node_modules/dep1/built.txt").exists(), "dep1 is initially ignored");
    assert_eq!(
        fs::read_to_string(workspace.join("node_modules/dep2/built.txt")).ok().as_deref(),
        Some("v1"),
        "dep2 is built during install",
    );

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["approve-builds", "dep1@file:../dep1"])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(workspace.join("node_modules/dep1/built.txt")).ok().as_deref(),
        Some("v1"),
        "approved directory dependency is built",
    );
    assert_eq!(
        fs::read_to_string(workspace.join("node_modules/dep2/built.txt")).ok().as_deref(),
        Some("v1"),
        "previously built directory dependency must keep its build output",
    );

    drop(root);
}

#[test]
fn filtered_rebuild_with_shared_lockfile_keeps_other_directory_dep_build_output() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_directory_dep(root.path(), "dep1", "v1");
    write_directory_dep(root.path(), "dep2", "v1");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nallowBuilds:\n  \"dep1@file:../dep1\": true\n  \"dep2@file:../dep2\": true\n",
    )
    .expect("write workspace manifest");
    fs::write(workspace.join("package.json"), r#"{ "name": "root", "version": "1.0.0" }"#)
        .expect("write root manifest");
    for (pkg, dep) in [("app-a", "dep1"), ("app-b", "dep2")] {
        let dir = workspace.join("packages").join(pkg);
        fs::create_dir_all(&dir).expect("create workspace project");
        fs::write(
            dir.join("package.json"),
            serde_json::json!({
                "name": pkg,
                "version": "1.0.0",
                "dependencies": { dep: format!("file:../../../{dep}") },
            })
            .to_string(),
        )
        .expect("write workspace project manifest");
    }

    pacquet
        .with_args(["install", "--reporter=silent"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(workspace.join("packages/app-a/node_modules/dep1/built.txt"))
            .ok()
            .as_deref(),
        Some("v1"),
    );
    assert_eq!(
        fs::read_to_string(workspace.join("packages/app-b/node_modules/dep2/built.txt"))
            .ok()
            .as_deref(),
        Some("v1"),
    );

    write_directory_dep(root.path(), "dep1", "v2");

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["--filter", "app-a", "rebuild", "dep1", "--reporter=silent"])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(workspace.join("packages/app-a/node_modules/dep1/built.txt"))
            .ok()
            .as_deref(),
        Some("v2"),
        "selected project's directory dependency is rebuilt",
    );
    assert_eq!(
        fs::read_to_string(workspace.join("packages/app-b/node_modules/dep2/built.txt"))
            .ok()
            .as_deref(),
        Some("v1"),
        "unselected project's directory dependency keeps its build output",
    );

    drop(root);
}
