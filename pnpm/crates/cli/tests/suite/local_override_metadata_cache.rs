//! An override that points a registry package's dependency at a local
//! directory is a decision of one project. The metadata cache is shared by
//! every project on the machine, so it must keep the registry's manifest.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

#[test]
fn an_override_to_a_local_directory_is_not_written_to_the_metadata_cache() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, cache_dir, .. } = npmrc_info;

    write_fixture(&workspace);

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile.contains("'@pnpm.e2e/dep-of-pkg-with-1-dep': local-dep@file:local-dep"),
        "the override must apply to the install:\n{lockfile}",
    );

    let registry_name =
        pnpm_resolving_npm_resolver::mirror::get_registry_name(mock_instance.url()).unwrap();
    let mirror_path = cache_dir
        .join("v11")
        .join("metadata")
        .join(registry_name)
        .join("@pnpm.e2e/pkg-with-1-dep.jsonl");
    let mirror = fs::read_to_string(&mirror_path).expect("read the metadata cache file");
    assert!(!mirror.contains("local-dep"), "the metadata cache holds the override:\n{mirror}");
    assert!(
        mirror.contains(r#""@pnpm.e2e/dep-of-pkg-with-1-dep":"^100.0.0""#),
        "the metadata cache lost the registry's dependency range:\n{mirror}",
    );

    drop((root, mock_instance));
}

/// A project that depends on a registry package and overrides that
/// package's dependency with the local `local-dep` directory.
fn write_fixture(workspace: &Path) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::create_dir(workspace.join("local-dep")).expect("create local-dep");
    fs::write(
        workspace.join("local-dep/package.json"),
        serde_json::json!({ "name": "local-dep", "version": "1.0.0" }).to_string(),
    )
    .expect("write local-dep/package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml = fs::read_to_string(&workspace_yaml_path).unwrap_or_default();
    if !workspace_yaml.is_empty() && !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str(
        "overrides:\n  \"@pnpm.e2e/dep-of-pkg-with-1-dep\": file:./local-dep\n",
    );
    fs::write(&workspace_yaml_path, workspace_yaml).expect("update pnpm-workspace.yaml");
}
