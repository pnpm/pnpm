use super::{AddMockedRegistry, CommandExtra, CommandTempCwd, fs, pacquet_at};
use assert_cmd::assert::OutputAssertExt;
use pnpm_lockfile::Lockfile;
use std::path::Path;

const DEP: &str = "@pnpm.e2e/circular-iterator";

#[test]
fn unrelated_workspace_change_keeps_direct_dependencies_with_cyclic_subtrees() {
    assert_unrelated_change_keeps_versions(&["install"]);
}

#[test]
fn lockfile_only_keeps_direct_dependencies_with_cyclic_subtrees() {
    assert_unrelated_change_keeps_versions(&["install", "--lockfile-only"]);
}

fn assert_unrelated_change_keeps_versions(args: &[&str]) {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).unwrap();
    fs::write(&yaml_path, format!("{yaml}packages:\n  - packages/*\n")).unwrap();
    write_member(&workspace, "a", &serde_json::json!({ DEP: "^2.0.0" }));
    write_member(&workspace, "c", &serde_json::json!({}));
    pacquet_at(&workspace)
        .with_args(args)
        .assert()
        .success();
    assert_version(&workspace, "a", "^2.0.0", "2.0.1");

    write_member(&workspace, "b", &serde_json::json!({ DEP: "2.0.0" }));
    pacquet_at(&workspace)
        .with_args(args)
        .assert()
        .success();
    assert_version(&workspace, "a", "^2.0.0", "2.0.1");
    assert_version(&workspace, "b", "2.0.0", "2.0.0");

    write_member(&workspace, "c", &serde_json::json!({ "@pnpm.e2e/foo": "100.0.0" }));
    pacquet_at(&workspace)
        .with_args(args)
        .assert()
        .success();
    assert_version(&workspace, "a", "^2.0.0", "2.0.1");
    assert_version(&workspace, "b", "2.0.0", "2.0.0");

    drop((root, mock_instance));
}

#[test]
fn keeps_an_older_direct_version_until_update_or_dedupe() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).unwrap();
    fs::write(
        &yaml_path,
        format!("{yaml}packages:\n  - packages/*\nresolutionMode: lowest-direct\n"),
    )
    .unwrap();
    write_member(&workspace, "a", &serde_json::json!({ DEP: "^2.0.0" }));
    install(&workspace);
    assert_version(&workspace, "a", "^2.0.0", "2.0.0");
    fs::write(&yaml_path, format!("{yaml}packages:\n  - packages/*\n")).unwrap();
    write_member(&workspace, "b", &serde_json::json!({ DEP: "2.0.1" }));
    install(&workspace);
    assert_version(&workspace, "a", "^2.0.0", "2.0.0");

    write_member(&workspace, "c", &serde_json::json!({ "@pnpm.e2e/foo": "100.0.0" }));
    install(&workspace);
    assert_version(&workspace, "a", "^2.0.0", "2.0.0");
    assert_version(&workspace, "b", "2.0.1", "2.0.1");

    let locked = fs::read(workspace.join("pnpm-lock.yaml")).unwrap();
    pacquet_at(&workspace)
        .with_args(["--filter", "a", "update", DEP, "--lockfile-only"])
        .assert()
        .success();
    assert_version(&workspace, "a", "^2.0.1", "2.0.1");

    write_member(&workspace, "a", &serde_json::json!({ DEP: "^2.0.0" }));
    fs::write(workspace.join("pnpm-lock.yaml"), locked).unwrap();
    pacquet_at(&workspace)
        .with_args(["dedupe", "--lockfile-only"])
        .assert()
        .success();
    assert_version(&workspace, "a", "^2.0.0", "2.0.1");
    drop((root, mock_instance));
}

fn write_member(workspace: &Path, name: &str, dependencies: &serde_json::Value) {
    let dir = workspace.join("packages").join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.json"),
        serde_json::json!({
            "name": name, "private": true, "dependencies": dependencies,
        })
        .to_string(),
    )
    .unwrap();
}

fn install(workspace: &Path) {
    pacquet_at(workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
}

fn assert_version(workspace: &Path, member: &str, specifier: &str, version: &str) {
    let lockfile = Lockfile::load_wanted_from_dir(workspace).unwrap().unwrap();
    let dep = &lockfile.importers[&format!("packages/{member}")].dependencies.as_ref().unwrap()
        [&DEP.parse().unwrap()];
    assert_eq!(dep.specifier, specifier);
    assert_eq!(dep.version.to_string(), version);
}
