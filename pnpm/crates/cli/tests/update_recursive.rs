//! Recursive `pacquet update` integration tests, ported from
//! `pnpm11/installing/commands/test/update/recursive.ts`.
//!
//! The upstream file's other cases drive `latest` around with
//! `addDistTag` mid-test; the fixture registry serves `latest` as the
//! highest published version and has no per-run override, so those stay
//! unported. See pnpm/pnpm#12101.

pub mod _utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{ffi::OsStr, fs, path::Path, process::Command};
use tempfile::TempDir;

/// Published at 100.0.0, 100.1.0, and 101.0.0.
const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const FOO: &str = "@pnpm.e2e/foo";

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

/// Register `manifests` as workspace packages, each in its own
/// subdirectory. Appends to the `pnpm-workspace.yaml` the harness wrote,
/// which carries the mocked registry settings.
fn write_workspace(workspace: &Path, manifests: &[(&str, Value)]) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    let packages: Vec<String> = manifests.iter().map(|(name, _)| format!("  - '{name}'")).collect();
    fs::write(&yaml_path, format!("{yaml}packages:\n{}\n", packages.join("\n")))
        .expect("write pnpm-workspace.yaml");

    for (name, manifest) in manifests {
        let dir = workspace.join(name);
        fs::create_dir_all(&dir).expect("create project dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }
}

fn dep_spec(project_dir: &Path, name: &str) -> Option<String> {
    let manifest = PackageManifest::from_path(project_dir.join("package.json")).unwrap();
    manifest
        .dependencies([DependencyGroup::Prod])
        .find(|(key, _)| *key == name)
        .map(|(_, spec)| spec.to_string())
}

fn has_module(project_dir: &Path, name: &str) -> bool {
    project_dir.join("node_modules").join(name).exists()
}

fn installed_version(project_dir: &Path, name: &str) -> Option<String> {
    let manifest_path = project_dir.join("node_modules").join(name).join("package.json");
    let contents = fs::read_to_string(manifest_path).ok()?;
    let value: Value = serde_json::from_str(&contents).ok()?;
    value["version"].as_str().map(str::to_string)
}

/// Ports `recursive update`: a versioned selector reaches every project
/// that already depends on the package, and no project gains it.
#[test]
fn recursive_update_only_reaches_projects_that_have_the_dependency() {
    let (root, workspace, anchor) = setup();

    write_workspace(
        &workspace,
        &[
            (
                "project-1",
                json!({ "name": "project-1", "version": "1.0.0",
                "dependencies": { DEP: "100.0.0" } }),
            ),
            (
                "project-2",
                json!({ "name": "project-2", "version": "1.0.0",
                "dependencies": { FOO: "1.0.0" } }),
            ),
        ],
    );
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["-r", "update", &format!("{DEP}@100.1.0")]).assert().success();

    assert_eq!(
        installed_version(&workspace.join("project-1"), DEP).as_deref(),
        Some("100.1.0"),
        "the project that declares it should have been updated",
    );
    assert!(
        !has_module(&workspace.join("project-2"), DEP),
        "a project that never declared it must not gain it",
    );

    drop((root, anchor));
}

/// Ports `recursive update in workspace should not add new dependencies`:
/// naming a package no project depends on fails at `--depth 0` and adds
/// it nowhere.
#[test]
fn recursive_update_does_not_add_a_dependency_no_project_declares() {
    let (root, workspace, anchor) = setup();

    write_workspace(
        &workspace,
        &[
            ("project-1", json!({ "name": "project-1", "version": "1.0.0" })),
            ("project-2", json!({ "name": "project-2", "version": "1.0.0" })),
        ],
    );
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["-r", "update", "--depth", "0", DEP])
        .output()
        .expect("run pacquet update");

    assert!(!output.status.success(), "updating an undeclared package should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("None of the specified packages were found in the dependencies"),
        "stderr did not mention NO_PACKAGE_IN_DEPENDENCIES: {stderr}",
    );
    for project in ["project-1", "project-2"] {
        assert!(!has_module(&workspace.join(project), DEP), "{project} gained the dependency");
    }

    drop((root, anchor));
}

/// Ports `recursive update with aliased workspace dependency (#7975)`: a
/// dependency aliased onto a workspace package keeps its specifier and
/// stays linked, rather than being rewritten or dropped.
#[test]
fn recursive_update_keeps_an_aliased_workspace_dependency() {
    let (root, workspace, anchor) = setup();

    write_workspace(
        &workspace,
        &[
            (
                "project-1",
                json!({ "name": "project-1", "version": "1.0.0",
                "dependencies": { "pkg": "workspace:project-2@^" } }),
            ),
            ("project-2", json!({ "name": "project-2", "version": "1.0.0" })),
        ],
    );
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["-r", "update", "--depth", "0"]).assert().success();

    assert!(has_module(&workspace.join("project-1"), "pkg"), "the alias should stay linked");
    assert_eq!(
        dep_spec(&workspace.join("project-1"), "pkg").as_deref(),
        Some("workspace:project-2@^"),
        "the aliased workspace specifier should survive the update",
    );

    drop((root, anchor));
}
