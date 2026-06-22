use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;

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

    let files =
        json.get("files").expect("has 'files' object").as_object().expect("'files' is an object");
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
