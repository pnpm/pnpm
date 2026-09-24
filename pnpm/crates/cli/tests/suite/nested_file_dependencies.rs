//! End-to-end coverage for `file:` dependencies declared by a package
//! that was itself resolved from a local directory.
//!
//! Such a specifier is relative to the manifest that declares it, not to
//! the importer that pulled the chain in — pnpm's `parentPkg.rootDir`.
//! Covers pnpm/pnpm#13323.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

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
            "parent-pkg@file+++_03a936b22c43a1cf3e318266a6921640/node_modules/parent-pkg/index.js"
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
