//! The repeat-install fast path deciding from the workspace state's manifest
//! records, without reading the project manifests whose recorded stat still
//! holds.

pub use _utils::*;

use crate::_utils;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

/// The debug event the fast path logs when the records decided.
const DECIDED_FROM_RECORDS: &str = "the recorded manifests decided the install";

/// A workspace whose `a` depends on `b` through `workspace:*` and links its
/// command, beside a `packages/*` directory that holds no project.
fn write_workspace(workspace: &Path) {
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    yaml.push_str("packages:\n  - packages/*\n");
    fs::write(&workspace_yaml, yaml).expect("write pnpm-workspace.yaml");
    let write_manifest = |dir: &str, manifest: serde_json::Value| {
        fs::create_dir_all(workspace.join(dir)).expect("create the project directory");
        fs::write(workspace.join(dir).join("package.json"), manifest.to_string())
            .expect("write package.json");
    };
    write_manifest(".", serde_json::json!({ "name": "root", "private": true }));
    write_manifest(
        "packages/a",
        serde_json::json!({ "name": "a", "version": "1.0.0", "dependencies": { "b": "workspace:*" } }),
    );
    write_manifest(
        "packages/b",
        serde_json::json!({ "name": "b", "version": "1.0.0", "bin": { "b": "cli.js" } }),
    );
    fs::write(workspace.join("packages/b/cli.js"), "#!/usr/bin/env node\n").expect("write cli.js");
    fs::create_dir_all(workspace.join("packages/no-project")).expect("create packages/no-project");
}

/// `pnpm install` with the fast path's trace, returning its output.
fn traced_install(workspace: &Path, registry: &str) -> String {
    let output = pacquet_in(workspace)
        .with_env("PNPM_CONFIG_REGISTRY", registry)
        .with_env("TRACE", "pacquet::install=debug")
        .with_arg("install")
        .assert()
        .success()
        .get_output()
        .clone();
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

#[test]
fn an_unchanged_workspace_is_up_to_date_from_the_records() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();

    let output = traced_install(&workspace, &mock_instance.url());

    assert!(output.contains("Already up to date"), "{output}");
    assert!(output.contains(DECIDED_FROM_RECORDS), "{output}");
    drop((root, mock_instance));
}

/// An edit the lockfile still satisfies leaves the install up to date, but
/// the edited manifest has to be read to tell.
#[test]
fn an_edited_manifest_is_read_again() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    fs::write(
        workspace.join("packages/b/package.json"),
        serde_json::json!({
            "name": "b",
            "version": "1.0.0",
            "description": "edited",
            "bin": { "b": "cli.js" },
        })
        .to_string(),
    )
    .expect("edit packages/b/package.json");

    let output = traced_install(&workspace, &mock_instance.url());

    assert!(output.contains("Already up to date"), "{output}");
    assert!(!output.contains(DECIDED_FROM_RECORDS), "{output}");
    drop((root, mock_instance));
}

#[test]
fn a_new_project_is_read_and_installed() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    fs::write(
        workspace.join("packages/no-project/package.json"),
        serde_json::json!({ "name": "c", "version": "1.0.0", "dependencies": { "b": "workspace:*" } })
            .to_string(),
    )
    .expect("write packages/no-project/package.json");

    let output = traced_install(&workspace, &mock_instance.url());

    assert!(!output.contains(DECIDED_FROM_RECORDS), "{output}");
    assert!(workspace.join("packages/no-project/node_modules/b").exists(), "{output}");
    drop((root, mock_instance));
}

/// The records stand in for the dependencies the link check reads, so a
/// removed link of an unchanged project is still found.
#[test]
fn a_removed_workspace_link_is_relinked() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_workspace(&workspace);
    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .assert()
        .success();
    let link = workspace.join("packages/a/node_modules/b");
    pnpm_fs::remove_dirent(&link).expect("remove the workspace link");

    let output = traced_install(&workspace, &mock_instance.url());

    assert!(!output.contains(DECIDED_FROM_RECORDS), "{output}");
    assert!(output.contains("the recorded manifests cannot decide"), "{output}");
    assert!(output.contains("a direct dependency link is missing"), "{output}");
    assert!(link.exists(), "{output}");
    drop((root, mock_instance));
}
