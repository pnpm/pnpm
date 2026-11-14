use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, command_env::CommandTestExt};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn command(template: &Command) -> Command {
    Command::new(template.get_program())
        .without_ambient_pnpm_config()
        .with_current_dir(template.get_current_dir().unwrap())
}

fn write_project(dir: &Path, manifest: &Value) {
    fs::create_dir_all(dir).expect("create project dir");
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
}

#[test]
fn recursive_get_keys_projects_sharing_a_name_by_directory() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - \"packages/*\"\n")
        .expect("write pnpm-workspace.yaml");
    write_project(&workspace, &json!({ "name": "root", "version": "0.0.0" }));
    let packages = workspace.join("packages");
    write_project(&packages.join("a"), &json!({ "name": "pkg-a", "version": "1.0.0" }));
    write_project(&packages.join("b"), &json!({ "name": "pkg-a", "version": "2.3.0" }));
    write_project(&packages.join("c"), &json!({ "name": "pkg-c", "version": "0.1.0" }));

    let output = pacquet
        .with_args(["-r", "pkg", "get", "version"])
        .output()
        .expect("run pnpm -r pkg get");

    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report: Value = serde_json::from_slice(&output.stdout).expect("stdout must be JSON");
    let dir_key = |name: &str| {
        Path::new("packages")
            .join(name)
            .display()
            .to_string()
    };
    let mut expected = serde_json::Map::new();
    expected.insert("root".into(), json!({ "version": "0.0.0" }));
    expected.insert(dir_key("a"), json!({ "version": "1.0.0" }));
    expected.insert(dir_key("b"), json!({ "version": "2.3.0" }));
    expected.insert("pkg-c".into(), json!({ "version": "0.1.0" }));
    assert_eq!(report, Value::Object(expected));
    drop(root);
}

#[test]
fn pkg_get_single_and_recursive_do_not_leak_synthetic_runtime_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - \"packages/*\"\n")
        .expect("write pnpm-workspace.yaml");
    let runtime = json!({
        "name": "node",
        "version": "22.14.0",
        "onFail": "download",
    });
    write_project(&workspace, &json!({ "name": "root", "devEngines": { "runtime": runtime } }));
    write_project(
        &workspace.join("packages/a"),
        &json!({ "name": "pkg-a", "devEngines": { "runtime": runtime } }),
    );

    let single_get = command(&pacquet)
        .args(["pkg", "get", "devDependencies"])
        .output()
        .expect("run pnpm pkg get devDependencies");
    assert!(single_get.status.success(), "{}", String::from_utf8_lossy(&single_get.stderr));
    assert_eq!(String::from_utf8_lossy(&single_get.stdout), "");

    let recursive_get = command(&pacquet)
        .args(["-r", "pkg", "get"])
        .output()
        .expect("run pnpm -r pkg get");
    assert!(recursive_get.status.success(), "{}", String::from_utf8_lossy(&recursive_get.stderr));
    let report: Value = serde_json::from_slice(&recursive_get.stdout).expect("stdout must be JSON");
    assert!(report["root"].get("devDependencies").is_none());
    assert!(report["pkg-a"].get("devDependencies").is_none());
    drop(root);
}

#[test]
fn pkg_set_and_delete_preserve_runtime_and_empty_dependency_objects() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project(
        &workspace,
        &json!({
            "name": "root",
            "devEngines": {
                "runtime": {
                    "name": "node",
                    "version": "22.14.0",
                    "onFail": "download",
                },
            },
        }),
    );

    for args in [
        &["pkg", "set", "devEngines.runtime.version=24.0.0"][..],
        &["pkg", "set", "dependencies={}", "--json"],
        &["pkg", "delete", "devDependencies"],
    ] {
        let output = command(&pacquet)
            .args(args)
            .output()
            .expect("run pnpm pkg");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    }

    let saved: Value =
        serde_json::from_str(&fs::read_to_string(workspace.join("package.json")).unwrap()).unwrap();
    assert_eq!(saved["devEngines"]["runtime"]["version"], "24.0.0");
    assert_eq!(saved["dependencies"], json!({}));
    assert!(saved.get("devDependencies").is_none());
    drop(root);
}
