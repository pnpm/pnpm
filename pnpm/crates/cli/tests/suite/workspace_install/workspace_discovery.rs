use super::{CommandExtra, CommandTempCwd, Path, assert_eq, fs, pacquet_at};
use assert_cmd::assert::OutputAssertExt;

fn workspace(workspace: &Path) {
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - 'packages/*'\nsharedWorkspaceLockfile: false\nworkspaceConcurrency: 1\n",
    )
    .unwrap();
    fs::write(workspace.join("package.json"), r#"{"name":"root","private":true}"#).unwrap();
    for (name, dependencies) in
        [("lib", serde_json::json!({})), ("app", serde_json::json!({ "lib": "workspace:*" }))]
    {
        let dir = workspace.join("packages").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("package.json"),
            serde_json::json!({
                "name": name, "version": "1.0.0", "dependencies": dependencies,
            })
            .to_string(),
        )
        .unwrap();
    }
    let dist = workspace.join("packages/lib/dist");
    fs::create_dir_all(&dist).unwrap();
    fs::write(dist.join("package.json"), r#"{"name":"lib","version":"1.0.0"}"#).unwrap();
}

fn assert_lib_target(workspace: &Path, directory: &str) {
    assert_eq!(
        fs::canonicalize(workspace.join("packages/app/node_modules/lib")).unwrap(),
        fs::canonicalize(workspace.join("packages/lib").join(directory)).unwrap(),
    );
    let lockfile = super::read_lockfile(&workspace.join("packages/app/pnpm-lock.yaml"));
    assert_eq!(lockfile.importers.len(), 1);
    assert!(lockfile.importers.contains_key("."));
}

#[test]
fn filtered_dedicated_install_resolves_unselected_siblings_and_reloads_next_command() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    workspace(&fixture.workspace);
    pacquet_at(&fixture.workspace)
        .with_args(["install", "--filter", "app", "--ignore-scripts"])
        .assert()
        .success();
    assert_lib_target(&fixture.workspace, "");
    assert!(!fixture.workspace.join("packages/lib/pnpm-lock.yaml").exists());

    let manifest_path = fixture.workspace.join("packages/lib/package.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["publishConfig"] = serde_json::json!({ "directory": "dist" });
    fs::write(manifest_path, manifest.to_string()).unwrap();
    // Resolve again without the previous lockfile's workspace link target.
    fs::remove_file(fixture.workspace.join("packages/app/pnpm-lock.yaml")).unwrap();
    fs::remove_dir_all(fixture.workspace.join("packages/app/node_modules")).unwrap();
    pacquet_at(&fixture.workspace)
        .with_args(["install", "--filter", "app", "--ignore-scripts", "--force"])
        .assert()
        .success();
    assert_lib_target(&fixture.workspace, "dist");
}

#[test]
fn dedicated_install_without_scripts_injects_workspace_dependencies() {
    let fixture = CommandTempCwd::init().add_mocked_registry();
    workspace(&fixture.workspace);
    fs::write(
        fixture.workspace.join("packages/app/package.json"),
        serde_json::json!({
            "name": "app", "version": "1.0.0",
            "dependencies": { "lib": "workspace:*" },
            "dependenciesMeta": { "lib": { "injected": true } },
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        fixture.workspace.join("packages/lib/package.json"),
        serde_json::json!({
            "name": "lib", "version": "1.0.0", "dependencies": { "is-positive": "1.0.0" },
        })
        .to_string(),
    )
    .unwrap();
    pacquet_at(&fixture.workspace)
        .with_args(["install", "--ignore-scripts", "--config.dedupe-injected-deps=false"])
        .assert()
        .success();
    let injected = fixture.workspace.join("packages/app/node_modules/lib");
    assert_ne!(
        fs::canonicalize(&injected).unwrap(),
        fs::canonicalize(fixture.workspace.join("packages/lib")).unwrap(),
    );
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(injected.join("package.json")).unwrap()).unwrap();
    assert_eq!(manifest["dependencies"]["is-positive"], "1.0.0");
    let installed = fs::canonicalize(&injected).unwrap();
    assert!(
        installed
            .parent()
            .unwrap()
            .join("is-positive/package.json")
            .exists(),
    );
}
