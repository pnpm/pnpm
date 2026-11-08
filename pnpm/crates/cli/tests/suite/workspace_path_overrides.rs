//! An override written as a `workspace:` path, such as
//! `workspace:./packages/target`, is written from the workspace root but
//! applied to projects anywhere below it.
//!
//! Covers <https://github.com/pnpm/pnpm/issues/17010>.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use pretty_assertions::assert_eq;
use std::{fs, path::Path, process::Command};

#[test]
fn workspace_path_override_links_the_workspace_relative_target() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    write_fixture(
        &workspace,
        &FixtureOptions {
            inject_workspace_packages: false,
            shared_workspace_lockfile: true,
            override_specifier: "workspace:./packages/target",
        },
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let consumer = workspace.join("packages/deep/consumer");
    assert_eq!(
        fs::canonicalize(consumer.join("node_modules/target")).expect("resolve the link"),
        fs::canonicalize(workspace.join("packages/target")).expect("resolve the target"),
    );

    drop((root, npmrc_info));
}

#[test]
fn workspace_path_override_installs_the_workspace_relative_target_when_injected() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    write_fixture(
        &workspace,
        &FixtureOptions {
            inject_workspace_packages: true,
            shared_workspace_lockfile: true,
            override_specifier: "workspace:./packages/target",
        },
    );

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let installed = workspace.join("packages/deep/consumer/node_modules/target/index.js");
    assert_eq!(
        fs::read_to_string(&installed).expect("read the installed package"),
        "module.exports = 'target'\n",
    );

    drop((root, npmrc_info));
}

#[test]
fn root_relative_overrides_resolve_from_workspace_config_with_both_lockfile_modes() {
    let mut failures = Vec::new();
    for shared_workspace_lockfile in [true, false] {
        for specifier in ["file:./packages/target", "workspace:./packages/target"] {
            let CommandTempCwd {
                pacquet,
                root,
                workspace,
                npmrc_info,
                ..
            } = CommandTempCwd::init().add_mocked_registry();
            write_fixture(
                &workspace,
                &FixtureOptions {
                    inject_workspace_packages: true,
                    shared_workspace_lockfile,
                    override_specifier: specifier,
                },
            );

            let install = pacquet
                .with_args(["install", "--offline", "--no-frozen-lockfile"])
                .output()
                .expect("run install");
            if !install.status.success() {
                failures.push(format!(
                    "sharedWorkspaceLockfile={shared_workspace_lockfile}, {specifier}: {install:?}",
                ));
                drop((root, npmrc_info));
                continue;
            }

            let consumer = workspace.join("packages/deep/consumer");
            let runtime = Command::new("node")
                .args(["-p", "require('target')"])
                .current_dir(&consumer)
                .output()
                .expect("run Node.js");
            if !runtime.status.success() || runtime.stdout != b"target\n" {
                failures.push(format!(
                    "sharedWorkspaceLockfile={shared_workspace_lockfile}, {specifier}: {runtime:?}",
                ));
            }

            drop((root, npmrc_info));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

struct FixtureOptions<'a> {
    inject_workspace_packages: bool,
    shared_workspace_lockfile: bool,
    override_specifier: &'a str,
}

fn write_fixture(workspace: &Path, options: &FixtureOptions<'_>) {
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!(
            "{workspace_yaml}\npackages:\n  - packages/**\n\
             injectWorkspacePackages: {}\n\
             sharedWorkspaceLockfile: {}\n\
             overrides:\n  target: {}\n",
            options.inject_workspace_packages,
            options.shared_workspace_lockfile,
            options.override_specifier,
        ),
    )
    .expect("write pnpm-workspace.yaml");

    write_manifest(workspace, &serde_json::json!({ "name": "root", "private": true }));
    let consumer = workspace.join("packages/deep/consumer");
    fs::create_dir_all(&consumer).expect("create the consumer");
    write_manifest(
        &consumer,
        &serde_json::json!({
            "name": "consumer",
            "version": "1.0.0",
            "dependencies": { "target": "1.0.0" },
        }),
    );
    let target = workspace.join("packages/target");
    fs::create_dir_all(&target).expect("create the target");
    write_manifest(
        &target,
        &serde_json::json!({ "name": "target", "version": "1.0.0", "main": "index.js" }),
    );
    fs::write(target.join("index.js"), "module.exports = 'target'\n").expect("write index.js");
}

fn write_manifest(dir: &Path, manifest: &serde_json::Value) {
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
}
