//! `extends` in `pnpm-workspace.yaml`: catalogs inherited from other
//! workspace manifests.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::Lockfile;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pretty_assertions::assert_eq;
use std::{fs, path::Path, process::Command};

pub(crate) const FOO: &str = "@pnpm.e2e/foo";

fn pacquet_install(workspace: &Path) -> std::process::Output {
    pacquet_ok(workspace, &["install"])
}

/// Run pnpm with `args` in `dir`, which must succeed.
pub(crate) fn pacquet_ok(dir: &Path, args: &[&str]) -> std::process::Output {
    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(dir)
        .with_args(args)
        .output()
        .expect("run pnpm");
    assert!(output.status.success(), "pnpm {args:?} failed: {output:?}");
    output
}

/// A project named `name` in `dir` that takes `@pnpm.e2e/foo` from the
/// default catalog.
pub(crate) fn write_package(dir: &Path, name: &str) {
    fs::create_dir_all(dir).expect("create the project dir");
    fs::write(
        dir.join("package.json"),
        serde_json::json!({ "name": name, "version": "1.0.0", "dependencies": { FOO: "catalog:" } })
            .to_string(),
    )
    .expect("write package.json");
}

pub(crate) fn write_workspace_manifest(dir: &Path, yaml: &str) {
    fs::create_dir_all(dir).expect("create the manifest dir");
    fs::write(dir.join("pnpm-workspace.yaml"), yaml).expect("write pnpm-workspace.yaml");
}

pub(crate) fn append_workspace_yaml(workspace: &Path, extra: &str) {
    let path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str(extra);
    fs::write(&path, yaml).expect("write pnpm-workspace.yaml");
}

/// The `(specifier, version)` the lockfile's default catalog records for foo.
pub(crate) fn foo_catalog_snapshot(workspace: &Path) -> (String, String) {
    let text = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    let lockfile: Lockfile = serde_saphyr::from_str(&text).expect("parse pnpm-lock.yaml");
    let entry = lockfile.catalogs
        .as_ref()
        .and_then(|catalogs| catalogs.get("default"))
        .and_then(|catalog| catalog.get(FOO))
        .unwrap_or_else(|| panic!("no default catalog entry for {FOO}:\n{text}"));
    (entry.specifier.clone(), entry.version.clone())
}

pub(crate) fn pair(specifier: &str, version: &str) -> (String, String) {
    (specifier.to_string(), version.to_string())
}

/// A `catalog:` dependency resolves against a catalog the workspace
/// manifest inherits, and `extends` is a setting pnpm knows.
#[test]
fn a_catalog_entry_comes_from_an_extended_manifest() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_manifest(
        &root.path().join("shared"),
        &format!("catalog:\n  '{FOO}': 100.0.0\n"),
    );
    append_workspace_yaml(&workspace, "extends: ../shared\n");
    write_package(&workspace, "app");

    let output = pacquet_install(&workspace);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("extends"), "extends must be a known setting:\n{stderr}");
    assert_eq!(foo_catalog_snapshot(&workspace), pair("100.0.0", "100.0.0"));
    drop((root, mock_instance));
}

/// The workspace manifest's own catalog entry wins over the inherited one.
#[test]
fn a_declared_catalog_entry_overrides_an_inherited_one() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_manifest(
        &root.path().join("shared"),
        &format!("catalog:\n  '{FOO}': 100.0.0\n"),
    );
    append_workspace_yaml(&workspace, &format!("extends: ../shared\ncatalog:\n  '{FOO}': 1.0.0\n"));
    write_package(&workspace, "app");

    pacquet_install(&workspace);

    assert_eq!(foo_catalog_snapshot(&workspace), pair("1.0.0", "1.0.0"));
    drop((root, mock_instance));
}

/// A change to an extended manifest reaches the next install, which must
/// not take the unchanged workspace manifest for an up-to-date install.
#[test]
fn a_changed_extended_manifest_is_resolved_again() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let shared = root.path().join("shared");
    write_workspace_manifest(&shared, &format!("catalog:\n  '{FOO}': 100.0.0\n"));
    append_workspace_yaml(&workspace, "extends: ../shared\n");
    write_package(&workspace, "app");
    pacquet_install(&workspace);

    write_workspace_manifest(&shared, &format!("catalog:\n  '{FOO}': 100.1.0\n"));
    pacquet_install(&workspace);

    assert_eq!(foo_catalog_snapshot(&workspace), pair("100.1.0", "100.1.0"));
    drop((root, mock_instance));
}

/// A glob gathers the catalogs of the workspace's own projects' manifests
/// into the workspace.
#[test]
fn a_workspace_can_extend_the_manifests_of_its_projects() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace_manifest(
        &workspace.join("packages/app"),
        &format!("catalog:\n  '{FOO}': 100.0.0\n"),
    );
    write_package(&workspace.join("packages/app"), "app");
    append_workspace_yaml(&workspace, "packages:\n  - packages/*\nextends: packages/*\n");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write the root package.json");

    pacquet_install(&workspace);

    assert_eq!(foo_catalog_snapshot(&workspace), pair("100.0.0", "100.0.0"));
    drop((root, mock_instance));
}
