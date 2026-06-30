use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{fs, process::Command};

#[test]
fn should_cat_index_of_installed_package() {
    let CommandTempCwd { pacquet, workspace, root, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let pacquet = pacquet;
    pacquet.with_args(["add", "@pnpm.e2e/hello-world-js-bin-parent"]).assert().success();

    let mut pacquet2 = std::process::Command::cargo_bin("pacquet").unwrap();
    pacquet2.current_dir(&workspace);
    let output = pacquet2
        .with_args(["cat-index", "@pnpm.e2e/hello-world-js-bin-parent"])
        .output()
        .expect("run pacquet cat-index");

    assert!(output.status.success(), "Failed to cat-index");

    let stdout = String::from_utf8(output.stdout).expect("valid utf8");
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");

    let files = files_from_cat_index(&json);
    assert_package_json_file_entry(files);

    drop(root);
}

#[test]
fn should_cat_index_of_npm_alias() {
    let CommandTempCwd { pacquet, workspace, root, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let alias_spec = "my-alias@npm:@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0";
    pacquet.with_args(["add", alias_spec]).assert().success();

    let mut pacquet2 = std::process::Command::cargo_bin("pacquet").unwrap();
    pacquet2.current_dir(&workspace);
    let output =
        pacquet2.with_args(["cat-index", alias_spec]).output().expect("run pacquet cat-index");

    assert!(output.status.success(), "Failed to cat-index npm alias");

    let stdout = String::from_utf8(output.stdout).expect("valid utf8");
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let files = files_from_cat_index(&json);
    assert_package_json_file_entry(files);

    drop(root);
}

#[test]
fn should_cat_index_with_dir_pointing_to_workspace_project() {
    let CommandTempCwd { pacquet, workspace, root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let dependency = "@pnpm.e2e/hello-world-js-bin-parent";

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write root package.json");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "storeDir: ../pacquet-store\ncacheDir: ../pacquet-cache\nenableGlobalVirtualStore: false\npackages:\n  - 'packages/*'\n",
    )
    .expect("write pnpm-workspace.yaml");

    let project_dir = workspace.join("packages/foo");
    fs::create_dir_all(&project_dir).expect("mkdir workspace project");
    fs::write(
        project_dir.join("package.json"),
        serde_json::json!({
            "name": "@local/foo",
            "version": "1.0.0",
            "private": true,
            "dependencies": { dependency: "1.0.0" },
        })
        .to_string(),
    )
    .expect("write workspace project package.json");

    pacquet.with_args(["install"]).assert().success();

    let project_dir_arg = project_dir.to_string_lossy().into_owned();
    let mut pacquet2 = Command::cargo_bin("pacquet").unwrap();
    pacquet2.current_dir(&workspace);
    let output = pacquet2
        .with_args(["--dir", project_dir_arg.as_str(), "cat-index", dependency])
        .output()
        .expect("run pacquet cat-index with --dir");

    assert!(
        output.status.success(),
        "Failed to cat-index with --dir: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8(output.stdout).expect("valid utf8");
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let files = files_from_cat_index(&json);
    assert_package_json_file_entry(files);

    drop(root);
}

#[test]
fn should_fail_on_missing_package() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init().add_mocked_registry();

    let output = pacquet
        .with_args(["cat-index", "@pnpm.e2e/hello-world-js-bin-parent"])
        .output()
        .expect("run pacquet cat-index");
    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No corresponding index file found"));

    drop(root);
}

fn files_from_cat_index(json: &serde_json::Value) -> &serde_json::Map<String, serde_json::Value> {
    json.get("files").expect("has 'files' object").as_object().expect("'files' is an object")
}

fn assert_package_json_file_entry(files: &serde_json::Map<String, serde_json::Value>) {
    let package_json = files
        .get("package.json")
        .expect("package.json must be in the index")
        .as_object()
        .expect("the package.json entry is an object");
    for key in ["digest", "mode", "size"] {
        assert!(
            package_json.contains_key(key),
            "the package.json file entry records {key:?}, got: {package_json:?}",
        );
    }
}
