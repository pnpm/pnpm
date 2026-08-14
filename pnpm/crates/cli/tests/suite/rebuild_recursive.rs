use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{fs, path::Path, process::Command};

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

    pacquet.with_args(["install", "--ignore-scripts", "--reporter=silent"]).assert().success();
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
fn dedicated_recursive_rebuild_no_bail_continues_topological_chunks() {
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

    pacquet.with_args(["install", "--ignore-scripts", "--reporter=silent"]).assert().success();
    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["-r", "--no-bail", "rebuild", "--pending", "--reporter=silent"])
        .output()
        .expect("run recursive rebuild");

    assert!(!output.status.success(), "the failed project should fail the command");
    assert!(
        app_b.join("rebuilt.txt").exists(),
        "--no-bail should continue to the dependent project's topological chunk: {output:?}",
    );

    drop(root);
}
