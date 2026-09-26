//! `metadataCache: false` keeps registry metadata off disk: the resolvers
//! neither read the mirror under the cache directory nor write to it.

use crate::_utils;

use _utils::pacquet_in;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

#[test]
fn no_metadata_cache_flag_writes_nothing_to_the_mirror() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, cache_dir, .. } = npmrc_info;
    write_manifest(&workspace);

    pacquet
        .with_args(["install", "--no-metadata-cache"])
        .assert()
        .success();

    assert!(workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep").exists());
    assert_eq!(metadata_mirrors(&cache_dir), Vec::<String>::new());

    drop((root, mock_instance));
}

#[test]
fn add_with_no_metadata_cache_flag_writes_nothing_to_the_mirror() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, cache_dir, .. } = npmrc_info;

    pacquet
        .with_args(["add", "@pnpm.e2e/pkg-with-1-dep", "--no-metadata-cache"])
        .assert()
        .success();

    assert!(workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep").exists());
    assert_eq!(metadata_mirrors(&cache_dir), Vec::<String>::new());

    drop((root, mock_instance));
}

#[test]
fn metadata_cache_setting_in_the_workspace_manifest_writes_nothing_to_the_mirror() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, cache_dir, .. } = npmrc_info;
    write_manifest(&workspace);
    append_workspace_setting(&workspace, "metadataCache: false");

    pacquet
        .with_arg("install")
        .assert()
        .success();

    assert!(workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep").exists());
    assert_eq!(metadata_mirrors(&cache_dir), Vec::<String>::new());

    drop((root, mock_instance));
}

#[test]
fn no_metadata_cache_flag_does_not_read_the_mirror() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, cache_dir, .. } = npmrc_info;
    write_manifest(&workspace);

    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert_ne!(metadata_mirrors(&cache_dir), Vec::<String>::new());
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    let output = pacquet_in(&workspace)
        .with_args(["install", "--offline", "--no-metadata-cache"])
        .assert()
        .failure()
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let message = stderr
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        message.contains("Failed to resolve @pnpm.e2e/pkg-with-1-dep@100.0.0"),
        "an offline install that skips the mirror must find no metadata:\n{stderr}",
    );

    drop((root, mock_instance));
}

#[test]
fn lockfile_verification_with_no_metadata_cache_flag_writes_nothing_to_the_mirror() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, cache_dir, .. } = npmrc_info;
    write_manifest(&workspace);
    // Any `minimumReleaseAge` turns on the lockfile verifier, which fetches
    // the packuments the lockfile names.
    append_workspace_setting(&workspace, "minimumReleaseAge: 1");

    pacquet
        .with_args(["install", "--lockfile-only", "--no-metadata-cache"])
        .assert()
        .success();
    assert_eq!(metadata_mirrors(&cache_dir), Vec::<String>::new());
    // The resolving install recorded the lockfile as verified; forget that
    // so the frozen install verifies it again.
    fs::remove_file(cache_dir.join("lockfile-verified.jsonl")).expect("remove verification record");

    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile", "--no-metadata-cache"])
        .assert()
        .success();

    assert!(workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep").exists());
    assert_eq!(metadata_mirrors(&cache_dir), Vec::<String>::new());

    drop((root, mock_instance));
}

fn write_manifest(workspace: &Path) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
}

fn append_workspace_setting(workspace: &Path, setting: &str) {
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml = fs::read_to_string(&workspace_yaml_path).unwrap_or_default();
    if !workspace_yaml.is_empty() && !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str(setting);
    workspace_yaml.push('\n');
    fs::write(&workspace_yaml_path, workspace_yaml).expect("update pnpm-workspace.yaml");
}

/// The `v11/metadata*` mirror directories under `cache_dir`.
fn metadata_mirrors(cache_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(cache_dir.join("v11")) else {
        return Vec::new();
    };
    let mut mirrors: Vec<String> = entries
        .map(|entry| {
            entry
                .expect("read cache entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.starts_with("metadata"))
        .collect();
    mirrors.sort();
    mirrors
}
