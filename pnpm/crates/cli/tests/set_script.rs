use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_package_manifest::PackageManifest;
use pacquet_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pacquet binary").with_current_dir(workspace)
}

fn write_manifest(workspace: &Path, value: &Value) {
    fs::write(workspace.join("package.json"), value.to_string()).expect("write package.json");
}

fn scripts(workspace: &Path) -> Value {
    PackageManifest::from_path(workspace.join("package.json"))
        .expect("read package.json")
        .value()
        .get("scripts")
        .cloned()
        .unwrap_or(Value::Null)
}

#[test]
fn exposes_the_ss_alias() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &json!({ "name": "test-package", "version": "1.0.0" }));

    pacquet.with_args(["ss", "build", "tsc"]).assert().success();

    assert_eq!(scripts(&workspace)["build"], json!("tsc"));
    drop(root);
}

#[test]
fn adds_a_script_when_none_exist() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &json!({ "name": "test-package", "version": "1.0.0" }));

    pacquet.with_args(["set-script", "build", "tsc -b"]).assert().success();

    assert_eq!(scripts(&workspace), json!({ "build": "tsc -b" }));
    drop(root);
}

#[test]
fn overwrites_an_existing_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &json!({ "name": "test-package", "scripts": { "build": "old" } }));

    pacquet.with_args(["set-script", "build", "tsc -b"]).assert().success();

    assert_eq!(scripts(&workspace)["build"], json!("tsc -b"));
    drop(root);
}

#[test]
fn joins_remaining_params_into_the_command() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &json!({ "name": "test-package", "version": "1.0.0" }));

    pacquet.with_args(["set-script", "lint", "eslint", "--fix", "src"]).assert().success();

    assert_eq!(scripts(&workspace)["lint"], json!("eslint --fix src"));
    drop(root);
}

#[test]
fn accepts_script_names_with_dots_hyphens_and_quotes() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &json!({ "name": "test-package", "version": "1.0.0" }));

    pacquet.with_args(["set-script", "my-build", "tsc -b"]).assert().success();
    pacquet_at(&workspace).with_args(["set-script", "pre.publish", "echo"]).assert().success();
    pacquet_at(&workspace)
        .with_args(["set-script", r#"weird"name"#, "echo", "weird"])
        .assert()
        .success();

    let scripts = scripts(&workspace);
    assert_eq!(scripts["my-build"], json!("tsc -b"));
    assert_eq!(scripts["pre.publish"], json!("echo"));
    assert_eq!(scripts[r#"weird"name"#], json!("echo weird"));
    drop(root);
}

#[test]
fn accepts_script_names_containing_an_equals_sign() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &json!({ "name": "test-package", "version": "1.0.0" }));

    pacquet.with_args(["set-script", "with=eq", "echo", "with=eq"]).assert().success();

    assert_eq!(scripts(&workspace)["with=eq"], json!("echo with=eq"));
    drop(root);
}

#[test]
fn fails_when_arguments_are_missing() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &json!({ "name": "test-package", "version": "1.0.0" }));

    let output =
        pacquet.with_args(["set-script", "build"]).output().expect("spawn pacquet set-script");
    assert!(
        !output.status.success(),
        "a missing command must fail (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_SET_SCRIPT_MISSING_ARGS"),
        "stderr must name the missing-args diagnostic; got:\n{stderr}",
    );
    drop(root);
}

#[test]
fn fails_when_no_arguments_are_given() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &json!({ "name": "test-package", "version": "1.0.0" }));

    // No arguments at all exercises the missing-name branch, separate from the
    // missing-command branch covered above.
    let output = pacquet.with_args(["set-script"]).output().expect("spawn pacquet set-script");
    assert!(
        !output.status.success(),
        "no arguments at all must fail (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_SET_SCRIPT_MISSING_ARGS"),
        "stderr must name the missing-args diagnostic; got:\n{stderr}",
    );
    drop(root);
}

#[test]
fn rejects_unsafe_script_names() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &json!({ "name": "test-package", "version": "1.0.0" }));

    let output =
        pacquet.with_args(["set-script", "__proto__", "echo"]).output().expect("spawn pacquet");
    assert!(
        !output.status.success(),
        "an unsafe script name must fail (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_UNSAFE_PROPERTY_PATH_KEY"),
        "stderr must name the unsafe-key diagnostic; got:\n{stderr}",
    );
    drop(root);
}
