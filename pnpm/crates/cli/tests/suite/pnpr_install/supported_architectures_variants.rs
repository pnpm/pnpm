//! `supportedArchitectures` keeps every archive of a version with platform
//! variants that one of its platforms needs in the store, round-tripped
//! through an in-process pnpr: the host's variant is linked, another listed
//! platform's variant is fetched beside it, and the archive that targets
//! `any` is fetched for a listed platform no variant serves.

use super::{
    configure_pnpr_auth, declare_pnpr_registries,
    hosted::{host_targets, start_hosted_pnpr},
    pacquet_at, point_npmrc_registry_at,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, fs::get_all_files};
use serde_json::json;
use std::{fmt::Write as _, fs, path::Path};

/// A platform the host is not, which `supportedArchitectures` can name.
fn foreign_target() -> &'static str {
    if std::env::consts::OS == "linux" { "darwin-arm64" } else { "linux-x64" }
}

/// Publish `@acme/cli@1.0.0` with an `any` archive, a variant for the host,
/// and one for [`foreign_target`], each archive holding one file whose
/// content is its path.
fn publish(registry: &str, token: &str) {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "@acme/cli",
            "version": "1.0.0",
            "files": [],
            "publishConfig": {
                "variants": [
                    { "targets": ["any"], "files": ["portable/any"] },
                    { "targets": host_targets(), "files": ["native/host"] },
                    { "targets": [foreign_target()], "files": ["native/foreign"] },
                ],
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    for file in ["portable/any", "native/host", "native/foreign"] {
        let path = workspace.join(file);
        fs::create_dir_all(path.parent().expect("a file in a directory")).expect("create dir");
        fs::write(&path, file).expect("write file");
    }
    let authority = registry.trim_start_matches("http:");
    fs::write(
        workspace.join(".npmrc"),
        format!("registry={registry}\n{authority}:_authToken={token}\n"),
    )
    .expect("write .npmrc");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("registries:\n  '{registry}':\n    serverType: pnpr\n"),
    )
    .expect("write pnpm-workspace.yaml");
    pacquet_at(&workspace)
        .without_env("GITHUB_ACTIONS")
        .without_env("GITLAB_CI")
        .without_env("NPM_ID_TOKEN")
        .with_args(["publish", "--no-git-checks", "--publish-wait-timeout=0"])
        .assert()
        .success();
    drop(root);
}

/// The files of the archives an install of `@acme/cli` with
/// `supportedArchitectures` set to `platforms` keeps in its store, by their
/// content, after checking that the host's variant is linked.
fn stored_files(registry: &str, token: &str, platforms: &[&str]) -> Vec<String> {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    point_npmrc_registry_at(&npmrc_info.npmrc_path, registry);
    configure_pnpr_auth(&npmrc_info.npmrc_path, registry, token);
    fs::write(
        workspace.join("package.json"),
        json!({ "dependencies": { "@acme/cli": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    let manifest = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&manifest).expect("read pnpm-workspace.yaml");
    writeln!(yaml, "\nsupportedArchitectures: [{}]", platforms.join(", ")).expect(
        "write supportedArchitectures",
    );
    fs::write(&manifest, yaml).expect("write pnpm-workspace.yaml");
    declare_pnpr_registries(&workspace, &[registry]);

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let linked = workspace.join("node_modules/@acme/cli");
    assert!(linked.join("native/host").exists(), "the host's variant is linked");
    assert!(!linked.join("native/foreign").exists());
    let stored = archive_files(&npmrc_info.store_dir);
    drop((root, npmrc_info));
    stored
}

/// The content of every archive file in the store that one of the
/// published archives holds.
fn archive_files(store_dir: &Path) -> Vec<String> {
    let mut files: Vec<String> = get_all_files(store_dir)
        .into_iter()
        .filter_map(|path| fs::read_to_string(store_dir.join(path)).ok())
        .filter(|content| {
            ["portable/any", "native/host", "native/foreign"].contains(&content.as_str())
        })
        .collect();
    files.sort();
    files.dedup();
    files
}

#[test]
fn another_listed_platforms_variant_is_fetched_beside_the_hosts() {
    let (registry, token) = start_hosted_pnpr();
    publish(&registry, &token);
    assert_eq!(
        stored_files(&registry, &token, &["current", foreign_target()]),
        ["native/foreign", "native/host"],
        "the archive that targets any is needed by no listed platform",
    );
    assert_eq!(stored_files(&registry, &token, &["current"]), ["native/host"]);
}

#[test]
fn a_listed_platform_no_variant_serves_fetches_the_any_archive() {
    let (registry, token) = start_hosted_pnpr();
    publish(&registry, &token);
    assert_eq!(
        stored_files(&registry, &token, &["current", "win32-ia32"]),
        ["native/host", "portable/any"],
    );
}
