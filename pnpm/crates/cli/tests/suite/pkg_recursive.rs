use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{fs, path::Path};

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
