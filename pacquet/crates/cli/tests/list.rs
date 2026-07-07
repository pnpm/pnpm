use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};

fn write_workspace(workspace: &Path, manifests: &[(&str, Value)]) {
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "root",
            "version": "1.0.0",
            "private": true,
        })
        .to_string(),
    )
    .expect("write root package.json");
    for (name, manifest) in manifests {
        let dir = workspace.join("packages").join(name);
        fs::create_dir_all(&dir).expect("create package dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }
}

#[test]
fn recursive_list_depth_minus_one_json_lists_workspace_projects() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            (
                "project-1",
                json!({
                    "name": "project-1",
                    "version": "1.0.0",
                    "scripts": { ".test": "jest" },
                }),
            ),
            (
                "project-2",
                json!({
                    "name": "project-2",
                    "version": "1.0.0",
                    "scripts": { ".test": "jest" },
                }),
            ),
        ],
    );

    let output = pacquet
        .with_arg("-r")
        .with_arg("list")
        .with_arg("--depth")
        .with_arg("-1")
        .with_arg("--json")
        .output()
        .expect("spawn pacquet list");

    assert!(
        output.status.success(),
        "recursive list should succeed:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    let packages: Vec<Value> = serde_json::from_slice(&output.stdout).expect("parse list JSON");
    let names = packages
        .iter()
        .map(|pkg| pkg["name"].as_str().expect("package name").to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from(["project-1".to_string(), "project-2".to_string(), "root".to_string()]),
    );

    drop(root);
}
