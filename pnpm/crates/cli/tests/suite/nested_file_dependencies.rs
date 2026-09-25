//! End-to-end coverage for `file:` dependencies declared by a package
//! that was itself resolved from a local directory.
//!
//! Such a specifier is relative to the manifest that declares it, not to
//! the importer that pulled the chain in — pnpm's `parentPkg.rootDir`.
//! Covers pnpm/pnpm#13323.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::{Lockfile, PackageKey, PkgName};
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    command_env::CommandTestExt,
};
use std::{fs, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
}

fn write_manifest(dir: &Path, manifest: &serde_json::Value) {
    fs::create_dir_all(dir).expect("create the package directory");
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
}

/// `parent` sits next to `child` inside the importer, so the two
/// candidate bases disagree: `file:../child` lands on `child` from the
/// declaring manifest's directory, and outside the workspace from the
/// importer's.
#[test]
fn nested_file_dep_resolves_against_the_declaring_manifest() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        &serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "private": true,
            "dependencies": { "nested-parent": "file:./parent" },
        }),
    );
    write_manifest(
        &workspace.join("parent"),
        &serde_json::json!({
            "name": "nested-parent",
            "version": "1.0.0",
            "dependencies": { "nested-child": "file:../child" },
        }),
    );
    write_manifest(
        &workspace.join("child"),
        &serde_json::json!({ "name": "nested-child", "version": "1.0.0" }),
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("nested-child@file:child:"),
        "pnpm-lock.yaml should resolve file:../child against parent/:\n{lockfile}",
    );

    let installed = workspace.join(
        "node_modules/.pnpm/nested-parent@file+parent/node_modules/nested-child/package.json",
    );
    assert!(
        installed.is_file(),
        "nested-child should be installed into nested-parent's virtual-store slot at {}",
        installed.display(),
    );

    drop((root, mock_instance));
}

/// The vite layout from pnpm/pnpm#13323: a workspace project depends on
/// a sibling directory that in turn depends on another one a level up.
/// Both the resolved directory and the snapshot's reference to it must
/// match what pnpm writes — the reference drops the `<name>@` prefix
/// because the alias equals the package's own name.
#[test]
fn nested_file_dep_of_a_workspace_project_matches_the_pnpm_lockfile() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        &serde_json::json!({ "name": "ws-root", "version": "0.0.0", "private": true }),
    );

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    write_manifest(
        &workspace.join("packages/license"),
        &serde_json::json!({
            "name": "license",
            "version": "1.0.0",
            "dependencies": { "nested-parent": "file:./parent" },
        }),
    );
    write_manifest(
        &workspace.join("packages/license/parent"),
        &serde_json::json!({
            "name": "nested-parent",
            "version": "1.0.0",
            "dependencies": { "nested-child": "file:../child" },
        }),
    );
    write_manifest(
        &workspace.join("packages/license/child"),
        &serde_json::json!({ "name": "nested-child", "version": "1.0.0" }),
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("{directory: packages/license/child, type: directory}"),
        "pnpm-lock.yaml should keep the license/ path component of the nested dep:\n{lockfile}",
    );
    assert!(
        lockfile.contains("nested-child: file:packages/license/child"),
        "the snapshot should reference the nested dep without a self-alias prefix:\n{lockfile}",
    );

    drop((root, mock_instance));
}

/// Regression test for [pnpm/pnpm#4623](https://github.com/pnpm/pnpm/issues/4623).
#[test]
fn install_refreshes_transitive_dependencies_of_a_file_directory() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        &serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "private": true,
            "dependencies": { "tools-probe": "file:tools" },
        }),
    );
    write_manifest(
        &workspace.join("tools"),
        &serde_json::json!({
            "name": "tools-probe",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0" },
        }),
    );
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut config = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    if !config.ends_with('\n') {
        config.push('\n');
    }
    config.push_str("shamefullyHoist: true\n");
    fs::write(&workspace_yaml, config).expect("enable shamefullyHoist");

    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    write_manifest(
        &workspace.join("tools"),
        &serde_json::json!({
            "name": "tools-probe",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.1.0" },
        }),
    );

    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let lockfile = Lockfile::load_wanted_from_dir(&workspace)
        .expect("load the updated lockfile")
        .expect("the install writes a lockfile");
    let local_key: PackageKey = "tools-probe@file:tools".parse().expect("parse local package key");
    let transitive_name =
        PkgName::parse("@pnpm.e2e/dep-of-pkg-with-1-dep").expect("parse transitive package name");
    let installed_version = lockfile.snapshots
        .as_ref()
        .and_then(|snapshots| snapshots.get(&local_key))
        .and_then(|snapshot| snapshot.dependencies.as_ref())
        .and_then(|dependencies| dependencies.get(&transitive_name))
        .expect("the local package snapshot records its transitive dependency")
        .to_string();
    assert_eq!(installed_version, "100.1.0");

    let hoisted_manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join(
            "node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json",
        ))
        .expect("read the shamefully-hoisted transitive package"),
    )
    .expect("parse the hoisted package manifest");
    assert_eq!(hoisted_manifest["version"], "100.1.0");

    drop((root, mock_instance));
}

/// The layout from pnpm/pnpm#8101: a project depends on the directory
/// above it, so the dep path ends in `file:..`. Windows strips trailing
/// dots from path segments, so the virtual-store directory has to
/// escape them, without landing on the slot of a same-named package at
/// `file:++`.
#[test]
fn file_dep_on_the_parent_directory_gets_a_windows_safe_slot() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry {
        mock_instance, store_dir, cache_dir, ..
    } = npmrc_info;

    let parent = workspace.join("parent");
    let project = parent.join("quick-start");
    let plus = project.join("++");
    let package =
        serde_json::json!({ "name": "parent-pkg", "version": "1.0.0", "files": ["index.js"] });
    write_manifest(&parent, &package);
    fs::write(parent.join("index.js"), "module.exports = 'parent'\n").expect("write index.js");
    write_manifest(&plus, &package);
    fs::write(plus.join("index.js"), "module.exports = 'plus'\n").expect("write index.js");
    write_manifest(
        &project,
        &serde_json::json!({
            "name": "quick-start",
            "version": "1.0.0",
            "private": true,
            "dependencies": { "parent-pkg": "file:../", "plus-pkg": "file:./++" },
        }),
    );
    fs::write(project.join(".npmrc"), format!("registry={}\n", mock_instance.url()))
        .expect("write .npmrc");
    fs::write(
        project.join("pnpm-workspace.yaml"),
        format!(
            "storeDir: {}\ncacheDir: {}\nenableGlobalVirtualStore: false\n",
            serde_json::to_string(&store_dir).expect("serialize the store dir"),
            serde_json::to_string(&cache_dir).expect("serialize the cache dir"),
        ),
    )
    .expect("write pnpm-workspace.yaml");

    pacquet
        .with_current_dir(&project)
        .with_arg("install")
        .assert()
        .success();

    let virtual_store = project.join("node_modules/.pnpm");
    let read = |path: &str| {
        fs::read_to_string(virtual_store.join(path))
            .unwrap_or_else(|error| panic!("read {path}: {error}"))
    };
    assert_eq!(
        read(
            "parent-pkg@file+++_3cf6176c884f1541b42906b711973e2d/node_modules/parent-pkg/index.js"
        ),
        "module.exports = 'parent'\n",
    );
    assert_eq!(
        read("parent-pkg@file+++/node_modules/parent-pkg/index.js"),
        "module.exports = 'plus'\n",
    );
    assert_eq!(
        fs::read_to_string(project.join("node_modules/parent-pkg/index.js"))
            .expect("read node_modules/parent-pkg/index.js"),
        "module.exports = 'parent'\n",
    );

    drop((root, mock_instance));
}
