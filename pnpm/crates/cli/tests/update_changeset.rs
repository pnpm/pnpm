use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use serde_json::{Value, json};
use std::{ffi::OsStr, fs, path::Path, process::Command};
use tempfile::TempDir;

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";

fn setup() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    (root, workspace, npmrc_info)
}

fn pacquet(workspace: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_args(args)
}

fn write_manifest(project_dir: &Path, manifest: impl Into<Value>) {
    fs::create_dir_all(project_dir).expect("create project directory");
    fs::write(project_dir.join("package.json"), manifest.into().to_string())
        .expect("write package.json");
}

fn append_workspace_yaml(workspace: &Path, yaml: &str) {
    let path = workspace.join("pnpm-workspace.yaml");
    let mut contents = fs::read_to_string(&path).expect("read pnpm-workspace.yaml");
    if !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(yaml);
    fs::write(path, contents).expect("write pnpm-workspace.yaml");
}

fn write_changeset_config(workspace: &Path, config: impl Into<Value>) {
    let changeset_dir = workspace.join(".changeset");
    fs::create_dir_all(&changeset_dir).expect("create .changeset");
    fs::write(changeset_dir.join("config.json"), config.into().to_string())
        .expect("write changeset config");
}

fn generated_changesets(workspace: &Path) -> Vec<std::path::PathBuf> {
    let changeset_dir = workspace.join(".changeset");
    let Ok(entries) = fs::read_dir(changeset_dir) else { return Vec::new() };
    let mut paths = entries
        .map(|entry| entry.expect("read changeset entry").path())
        .filter(|path| {
            path.file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.starts_with("pnpm-update-") && name.ends_with(".md"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn generated_changeset_text(workspace: &Path) -> String {
    let paths = generated_changesets(workspace);
    assert_eq!(paths.len(), 1);
    fs::read_to_string(&paths[0]).expect("read generated changeset")
}

fn root_workspace_manifest() -> Value {
    json!({ "name": "workspace-root", "version": "1.0.0", "private": true })
}

#[test]
fn update_changeset_records_only_publishable_production_dependency_changes() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, root_workspace_manifest());
    append_workspace_yaml(&workspace, "packages:\n  - 'packages/*'\n");
    write_manifest(
        &workspace.join("packages/project-1"),
        json!({
            "name": "project-1",
            "version": "1.0.0",
            "dependencies": { (DEP): "^100.0.0" },
        }),
    );
    write_manifest(
        &workspace.join("packages/project-2"),
        json!({
            "name": "project-2",
            "version": "1.0.0",
            "devDependencies": { (DEP): "^100.0.0" },
        }),
    );
    write_manifest(
        &workspace.join("packages/project-3"),
        json!({
            "name": "project-3",
            "version": "1.0.0",
            "private": true,
            "dependencies": { (DEP): "^100.0.0" },
        }),
    );
    write_manifest(
        &workspace.join("packages/ignored-project"),
        json!({
            "name": "ignored-project",
            "version": "1.0.0",
            "dependencies": { (DEP): "^100.0.0" },
        }),
    );
    write_changeset_config(&workspace, json!({ "ignore": ["ignored-*"] }));

    pacquet(&workspace, ["-r", "install"]).assert().success();
    pacquet(&workspace, ["-r", "update", "--latest", "--changeset"]).assert().success();

    assert_eq!(
        generated_changeset_text(&workspace),
        "---\n\"project-1\": patch\n---\n\nUpdate dependencies.\n",
    );
    drop((root, anchor));
}

#[test]
fn update_changeset_records_peer_dependency_changes_as_major() {
    let (root, workspace, anchor) = setup();
    write_manifest(
        &workspace,
        json!({
            "name": "project",
            "version": "1.0.0",
            "dependencies": { (DEP): "catalog:" },
            "peerDependencies": { (DEP): "catalog:" },
        }),
    );
    append_workspace_yaml(&workspace, &format!("catalog:\n  '{DEP}': '^100.0.0'\n"));
    write_changeset_config(&workspace, json!({}));

    pacquet(&workspace, ["install"]).assert().success();
    pacquet(&workspace, ["update", "--latest", "--changeset"]).assert().success();

    assert_eq!(
        generated_changeset_text(&workspace),
        "---\n\"project\": major\n---\n\nUpdate dependencies.\n",
    );
    drop((root, anchor));
}

#[test]
fn update_changeset_skips_dev_dependency_only_changes() {
    let (root, workspace, anchor) = setup();
    write_manifest(
        &workspace,
        json!({
            "name": "project",
            "version": "1.0.0",
            "devDependencies": { (DEP): "^100.0.0" },
        }),
    );
    write_changeset_config(&workspace, json!({}));

    pacquet(&workspace, ["install"]).assert().success();
    pacquet(&workspace, ["update", "--latest", "--changeset"]).assert().success();

    assert!(generated_changesets(&workspace).is_empty());
    drop((root, anchor));
}

#[test]
fn update_changeset_records_every_consumer_of_a_changed_catalog_entry() {
    let (root, workspace, anchor) = setup();
    write_manifest(&workspace, root_workspace_manifest());
    append_workspace_yaml(
        &workspace,
        &format!("packages:\n  - 'packages/*'\ncatalog:\n  '{DEP}': '^100.0.0'\n"),
    );
    for name in ["project-1", "project-2"] {
        write_manifest(
            &workspace.join("packages").join(name),
            json!({
                "name": name,
                "version": "1.0.0",
                "dependencies": { (DEP): "catalog:" },
            }),
        );
    }
    write_manifest(
        &workspace.join("packages/project-3"),
        json!({
            "name": "project-3",
            "version": "1.0.0",
            "dependencies": { "is-positive": "1.0.0" },
        }),
    );
    write_changeset_config(&workspace, json!({}));

    pacquet(&workspace, ["-r", "install"]).assert().success();
    pacquet(&workspace, ["--filter", "project-1", "update", "--latest", "--changeset"])
        .assert()
        .success();

    let workspace_yaml =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace yaml");
    assert!(workspace_yaml.contains("^101.0.0"), "unexpected catalog: {workspace_yaml}");
    assert_eq!(
        generated_changeset_text(&workspace),
        "---\n\"project-1\": patch\n\"project-2\": patch\n---\n\nUpdate dependencies.\n",
    );
    drop((root, anchor));
}

#[test]
fn update_changeset_skips_resolution_only_catalog_movement() {
    let (root, workspace, anchor) = setup();
    write_manifest(
        &workspace,
        json!({
            "name": "project",
            "version": "1.0.0",
            "dependencies": { (DEP): "catalog:" },
        }),
    );
    append_workspace_yaml(&workspace, &format!("catalog:\n  '{DEP}': '100.0.0'\n"));
    write_changeset_config(&workspace, json!({}));
    pacquet(&workspace, ["install"]).assert().success();

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml = fs::read_to_string(&workspace_yaml_path)
        .expect("read workspace yaml")
        .replace("'100.0.0'", "'^100.0.0'");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("widen catalog range");
    pacquet(&workspace, ["update", "--no-save", "--changeset"]).assert().success();

    assert!(generated_changesets(&workspace).is_empty());
    drop((root, anchor));
}

#[test]
fn update_changeset_warns_and_skips_when_config_is_missing() {
    let (root, workspace, anchor) = setup();
    write_manifest(
        &workspace,
        json!({
            "name": "project",
            "version": "1.0.0",
            "dependencies": { (DEP): "^100.0.0" },
        }),
    );
    pacquet(&workspace, ["install"]).assert().success();

    let output =
        pacquet(&workspace, ["update", "--latest", "--changeset"]).output().expect("run update");
    assert!(output.status.success(), "update failed: {}", String::from_utf8_lossy(&output.stderr));
    let rendered = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        rendered.contains("No changeset was generated") && rendered.contains("config.json"),
        "missing-config warning was not printed: {rendered}",
    );
    assert!(!workspace.join(".changeset").exists());
    drop((root, anchor));
}

#[test]
fn update_config_changeset_enables_generation_by_default() {
    let (root, workspace, anchor) = setup();
    write_manifest(
        &workspace,
        json!({
            "name": "project",
            "version": "1.0.0",
            "dependencies": { (DEP): "^100.0.0" },
        }),
    );
    append_workspace_yaml(&workspace, "updateConfig:\n  changeset: true\n");
    write_changeset_config(&workspace, json!({}));

    pacquet(&workspace, ["install"]).assert().success();
    pacquet(&workspace, ["update", "--latest"]).assert().success();

    assert_eq!(
        generated_changeset_text(&workspace),
        "---\n\"project\": patch\n---\n\nUpdate dependencies.\n",
    );
    drop((root, anchor));
}

#[test]
fn no_changeset_overrides_update_config_changeset() {
    let (root, workspace, anchor) = setup();
    write_manifest(
        &workspace,
        json!({
            "name": "project",
            "version": "1.0.0",
            "dependencies": { (DEP): "^100.0.0" },
        }),
    );
    append_workspace_yaml(&workspace, "updateConfig:\n  changeset: true\n");
    write_changeset_config(&workspace, json!({}));

    pacquet(&workspace, ["install"]).assert().success();
    pacquet(&workspace, ["update", "--latest", "--no-changeset"]).assert().success();

    assert!(generated_changesets(&workspace).is_empty());
    drop((root, anchor));
}

#[test]
fn update_changeset_reports_malformed_config_with_a_stable_error() {
    let (root, workspace, anchor) = setup();
    write_manifest(
        &workspace,
        json!({
            "name": "project",
            "version": "1.0.0",
            "dependencies": { (DEP): "^100.0.0" },
        }),
    );
    pacquet(&workspace, ["install"]).assert().success();
    let changeset_dir = workspace.join(".changeset");
    fs::create_dir(&changeset_dir).expect("create .changeset");
    fs::write(changeset_dir.join("config.json"), "{").expect("write malformed config");

    let output =
        pacquet(&workspace, ["update", "--latest", "--changeset"]).output().expect("run update");
    assert!(!output.status.success(), "malformed config must fail the update");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_INVALID_CHANGESET_CONFIG") && stderr.contains("config.json"),
        "unexpected error: {stderr}",
    );
    drop((root, anchor));
}

#[test]
fn update_changeset_refuses_a_symlinked_changeset_directory() {
    let (root, workspace, anchor) = setup();
    write_manifest(
        &workspace,
        json!({
            "name": "project",
            "version": "1.0.0",
            "dependencies": { (DEP): "^100.0.0" },
        }),
    );
    pacquet(&workspace, ["install"]).assert().success();
    let outside_changeset_dir = root.path().join("outside-changeset");
    fs::create_dir(&outside_changeset_dir).expect("create outside changeset directory");
    fs::write(outside_changeset_dir.join("config.json"), "{}").expect("write changeset config");
    pacquet_fs::symlink_dir(&outside_changeset_dir, &workspace.join(".changeset"))
        .expect("link changeset directory outside workspace");

    let output =
        pacquet(&workspace, ["update", "--latest", "--changeset"]).output().expect("run update");
    assert!(!output.status.success(), "symlinked changeset directory must fail the update");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_UNSAFE_CHANGESET_DIR"), "unexpected error: {stderr}");
    assert_eq!(
        fs::read_dir(&outside_changeset_dir).expect("read outside changeset directory").count(),
        1,
    );
    drop((root, anchor));
}

#[test]
fn update_changeset_supports_a_package_outside_a_workspace() {
    let (root, workspace, anchor) = setup();
    fs::remove_file(workspace.join("pnpm-workspace.yaml")).expect("remove workspace manifest");
    write_manifest(
        &workspace,
        json!({
            "name": "project",
            "version": "1.0.0",
            "dependencies": { (DEP): "^100.0.0" },
        }),
    );
    write_changeset_config(&workspace, json!({}));

    pacquet(&workspace, ["install"]).assert().success();
    pacquet(&workspace, ["update", "--latest", "--changeset"]).assert().success();

    assert_eq!(
        generated_changeset_text(&workspace),
        "---\n\"project\": patch\n---\n\nUpdate dependencies.\n",
    );
    drop((root, anchor));
}
