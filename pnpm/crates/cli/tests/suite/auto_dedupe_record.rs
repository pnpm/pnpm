//! The record that lets a repeated `--lockfile-only` install skip resolving
//! a lockfile a deduplicating resolution already wrote.

use crate::auto_dedupe::{DEP, pnpm_at, write_project, write_settings};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

const PROJECTS: &str = "packages:\n  - low\n  - high\nautoDedupe: true\n";

/// Exact pins keep both versions, so every deduplicating resolution reopens
/// the package and needs its metadata.
fn write_exact_pins(workspace: &Path, settings: &str) {
    write_settings(workspace, format!("{PROJECTS}{settings}")).unwrap();
    write_project(workspace, "low", "100.0.0");
    write_project(workspace, "high", "100.1.0");
}

fn install(workspace: &Path, args: &[&str]) {
    pnpm_at(workspace)
        .with_args(["install", "--lockfile-only"])
        .with_args(args)
        .assert()
        .success();
}

/// Without registry metadata a resolution fails, so an offline install
/// succeeds only when it trusts the lockfile as it is.
fn resolves_offline_without_metadata(workspace: &Path, cache_dir: &Path, args: &[&str]) -> bool {
    // Every metadata mirror lives under the version directory of this one.
    let metadata_version_dir =
        Path::new(pnpm_resolving_npm_resolver::mirror::ABBREVIATED_META_DIR).parent().unwrap();
    let metadata = cache_dir.join(metadata_version_dir);
    if metadata.exists() {
        fs::remove_dir_all(metadata).unwrap();
    }
    let output = pnpm_at(workspace)
        .with_args(["install", "--lockfile-only", "--offline"])
        .with_args(args)
        .output()
        .unwrap();
    if output.status.success() {
        return false;
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Failed to resolve dependency tree"), "{stderr}");
    true
}

fn set_manifest_field(path: &Path, field: &str, value: serde_json::Value) {
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    manifest[field] = value;
    fs::write(path, manifest.to_string()).unwrap();
}

#[test]
fn repeat_lockfile_only_install_reuses_the_deduplicated_lockfile() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, cache_dir, .. } = npmrc_info;
    write_exact_pins(&workspace, "");
    install(&workspace, &[]);
    let lockfile = workspace.join("pnpm-lock.yaml");
    let deduplicated = fs::read(&lockfile).unwrap();
    assert!(!resolves_offline_without_metadata(&workspace, &cache_dir, &[]));
    assert_eq!(fs::read(&lockfile).unwrap(), deduplicated);

    set_manifest_field(&workspace.join("high/package.json"), "version", "1.0.1".into());
    assert!(resolves_offline_without_metadata(&workspace, &cache_dir, &[]));
    drop((root, mock_instance));
}

#[test]
fn a_filtered_install_does_not_record_its_lockfile() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, cache_dir, .. } = npmrc_info;
    write_exact_pins(&workspace, "");
    install(&workspace, &["--no-auto-dedupe"]);
    pnpm_at(&workspace)
        .with_args(["--filter", "low", "install", "--lockfile-only"])
        .assert()
        .success();
    assert!(resolves_offline_without_metadata(&workspace, &cache_dir, &[]));
    drop((root, mock_instance));
}

#[test]
fn a_changed_local_directory_misses_the_record() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, cache_dir, .. } = npmrc_info;
    write_exact_pins(&workspace, "");
    let local = workspace.join("local/package.json");
    fs::create_dir_all(workspace.join("local")).unwrap();
    fs::write(&local, serde_json::json!({"name": "local", "version": "1.0.0"}).to_string())
        .unwrap();
    let high = workspace.join("high/package.json");
    set_manifest_field(
        &high,
        "dependencies",
        serde_json::json!({DEP: "100.1.0", "local": "file:../local"}),
    );
    install(&workspace, &[]);
    assert!(!resolves_offline_without_metadata(&workspace, &cache_dir, &[]));

    set_manifest_field(&local, "description", "edited".into());
    assert!(resolves_offline_without_metadata(&workspace, &cache_dir, &[]));
    drop((root, mock_instance));
}

#[test]
fn a_changed_catalog_misses_the_record_although_the_lockfile_stays_fresh() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, cache_dir, .. } = npmrc_info;
    write_exact_pins(&workspace, "catalog:\n  unused: 1.0.0\n");
    install(&workspace, &[]);
    assert!(!resolves_offline_without_metadata(&workspace, &cache_dir, &[]));

    write_exact_pins(&workspace, "catalog:\n  unused: 2.0.0\n");
    assert!(resolves_offline_without_metadata(&workspace, &cache_dir, &[]));
    assert!(!resolves_offline_without_metadata(&workspace, &cache_dir, &["--no-auto-dedupe"]));
    drop((root, mock_instance));
}

#[test]
fn a_lockfile_an_after_all_resolved_hook_returned_is_not_recorded() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, cache_dir, .. } = npmrc_info;
    write_exact_pins(&workspace, "");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { afterAllResolved: (lockfile) => lockfile } }\n",
    )
    .unwrap();
    install(&workspace, &[]);
    assert!(resolves_offline_without_metadata(&workspace, &cache_dir, &[]));
    drop((root, mock_instance));
}

#[test]
fn dedicated_lockfiles_do_not_record() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, cache_dir, .. } = npmrc_info;
    write_settings(&workspace, format!("{PROJECTS}sharedWorkspaceLockfile: false\n")).unwrap();
    // A direct pin above the parent's `^100.0.0` keeps two versions in each
    // project's own lockfile.
    write_project(&workspace, "low", "101.0.0");
    write_project(&workspace, "high", "101.0.0");
    install(&workspace, &[]);
    assert!(workspace.join("low/pnpm-lock.yaml").exists());
    assert!(resolves_offline_without_metadata(&workspace, &cache_dir, &[]));
    drop((root, mock_instance));
}

#[test]
fn git_branch_lockfiles_do_not_record() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, cache_dir, .. } = npmrc_info;
    write_exact_pins(&workspace, "gitBranchLockfile: true\n");
    install(&workspace, &[]);
    assert!(resolves_offline_without_metadata(&workspace, &cache_dir, &[]));
    drop((root, mock_instance));
}
