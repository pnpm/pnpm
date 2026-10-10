use assert_cmd::prelude::*;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::fs;

#[test]
fn frozen_injected_installs_reuse_unchanged_hardlinks_and_refresh_replacements() {
    for global_store in [false, true] {
        let CommandTempCwd {
            mut pacquet, root: _root, workspace, ..
        } = CommandTempCwd::init();
        fs::write(
            workspace.join("package.json"),
            serde_json::json!({
                "name": "root", "private": true,
                "dependencies": { "library": "workspace:*" },
            })
            .to_string(),
        )
        .unwrap();
        fs::write(workspace.join("pnpm-workspace.yaml"), format!(
            "packages:\n  - packages/*\nstoreDir: ./store\ncacheDir: ./cache\ninjectWorkspacePackages: true\ndedupeInjectedDeps: false\nenableGlobalVirtualStore: {global_store}\n",
        )).unwrap();
        let source = workspace.join("packages/library");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("package.json"), r#"{"name":"library","version":"1.0.0"}"#).unwrap();
        fs::write(source.join("index.js"), "original").unwrap();
        pacquet
            .args(["install", "--offline", "--ignore-scripts", "--no-frozen-lockfile"])
            .assert()
            .success();
        let target = fs::canonicalize(workspace.join("node_modules/library")).unwrap();
        assert_ne!(target, fs::canonicalize(&source).unwrap());
        let original = same_file::Handle::from_path(&target).unwrap();
        let repeated = crate::_utils::pacquet_in(&workspace)
            .args(["install", "--offline", "--frozen-lockfile", "--reporter=ndjson"])
            .assert()
            .success();
        let stderr = String::from_utf8_lossy(&repeated.get_output().stderr);
        let methods: Vec<serde_json::Value> = stderr
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .filter(|event: &serde_json::Value| event["name"] == "pnpm:package-import-method")
            .collect();
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0]["method"], "hardlink");
        assert_eq!(same_file::Handle::from_path(&target).unwrap(), original);

        fs::write(source.join("replacement.js"), "modified").unwrap();
        fs::rename(source.join("replacement.js"), source.join("index.js")).unwrap();
        assert_eq!(fs::read_to_string(target.join("index.js")).unwrap(), "original");
        crate::_utils::pacquet_in(&workspace)
            .args(["install", "--offline", "--frozen-lockfile"])
            .assert()
            .success();
        assert_eq!(fs::read_to_string(target.join("index.js")).unwrap(), "modified");
        assert_ne!(same_file::Handle::from_path(&target).unwrap(), original);
        assert!(same_file::is_same_file(source.join("index.js"), target.join("index.js")).unwrap());
    }
}
