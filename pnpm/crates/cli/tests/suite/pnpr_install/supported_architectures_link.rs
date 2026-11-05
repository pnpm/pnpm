//! `supportedArchitectures` listing one platform other than the host links
//! that platform's variant of a package with platform variants, and the
//! dependencies that variant declares, round-tripped through an in-process
//! pnpr. Changing the setting relinks the package.

use super::{
    configure_pnpr_auth, declare_pnpr_registries,
    hosted::{host_targets, start_hosted_pnpr},
    pacquet_at, point_npmrc_registry_at,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{fmt::Write as _, fs, path::Path};

/// A platform the host is not, which `supportedArchitectures` can name.
fn foreign_target() -> &'static str {
    if std::env::consts::OS == "linux" { "darwin-arm64" } else { "linux-x64" }
}

/// Publish `manifest` to `registry`, its archive holding `files`, each
/// holding its own path.
fn publish(registry: &str, token: &str, manifest: &Value, files: &[&str]) {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
    for file in files {
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

/// Publish `@acme/dep@1.0.0`, and `@acme/cli@1.0.0` with an `any` archive,
/// a variant for the host, and one for [`foreign_target`] that depends on
/// `@acme/dep`.
fn publish_cli(registry: &str, token: &str) {
    publish(
        registry,
        token,
        &json!({ "name": "@acme/dep", "version": "1.0.0", "files": ["dep.js"] }),
        &["dep.js"],
    );
    let cli = json!({
        "name": "@acme/cli",
        "version": "1.0.0",
        "files": [],
        "publishConfig": {
            "variants": [
                { "targets": ["any"], "files": ["portable/any"] },
                { "targets": host_targets(), "files": ["native/host"] },
                {
                    "targets": [foreign_target()],
                    "files": ["native/foreign"],
                    "manifest": { "dependencies": { "@acme/dep": "1.0.0" } },
                },
            ],
        },
    });
    publish(registry, token, &cli, &["portable/any", "native/host", "native/foreign"]);
}

/// Set `supportedArchitectures` to the list `platforms` in `workspace`'s
/// manifest.
fn set_supported_architectures(workspace: &Path, platforms: &[&str]) {
    set_supported_architectures_to(workspace, &format!("[{}]", platforms.join(", ")));
}

/// Set `supportedArchitectures` to the YAML flow value `value` in
/// `workspace`'s manifest.
fn set_supported_architectures_to(workspace: &Path, value: &str) {
    let manifest = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&manifest).expect("read pnpm-workspace.yaml");
    let mut kept = String::new();
    for line in yaml
        .lines()
        .filter(|line| !line.starts_with("supportedArchitectures:"))
    {
        writeln!(kept, "{line}").expect("keep line");
    }
    writeln!(kept, "supportedArchitectures: {value}").expect("write supportedArchitectures");
    fs::write(&manifest, kept).expect("write pnpm-workspace.yaml");
}

/// A project depending on `@acme/cli` from `registry`.
fn consumer(
    registry: &str,
    token: &str,
) -> CommandTempCwd<pnpm_testing_utils::bin::AddMockedRegistry> {
    let command = CommandTempCwd::init().add_mocked_registry();
    point_npmrc_registry_at(&command.npmrc_info.npmrc_path, registry);
    configure_pnpr_auth(&command.npmrc_info.npmrc_path, registry, token);
    fs::write(
        command.workspace.join("package.json"),
        json!({ "dependencies": { "@acme/cli": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    declare_pnpr_registries(&command.workspace, &[registry]);
    command
}

#[test]
fn the_object_form_naming_one_platform_links_its_variant() {
    let (registry, token) = start_hosted_pnpr();
    publish_cli(&registry, &token);
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = consumer(&registry, &token);
    let (os, cpu) = foreign_target().split_once('-').expect("an os-cpu target");
    set_supported_architectures_to(&workspace, &format!("{{ os: [{os}], cpu: [{cpu}] }}"));

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let linked = workspace.join("node_modules/@acme/cli");
    assert!(linked.join("native/foreign").exists(), "the named platform's variant is linked");
    assert!(!linked.join("native/host").exists());
    drop((root, npmrc_info));
}

#[test]
fn one_listed_platform_links_its_variant_and_its_dependencies() {
    let (registry, token) = start_hosted_pnpr();
    publish_cli(&registry, &token);
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = consumer(&registry, &token);
    set_supported_architectures(&workspace, &[foreign_target()]);

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let linked = workspace.join("node_modules/@acme/cli");
    assert!(linked.join("native/foreign").exists(), "the listed platform's variant is linked");
    assert!(!linked.join("native/host").exists());
    assert!(
        linked.join("node_modules/@acme/dep/dep.js").exists()
            || workspace.join("node_modules/.pnpm/node_modules/@acme/dep/dep.js").exists(),
        "the dependency of the linked variant is installed",
    );

    // Changing the setting relinks the package from the same lockfile.
    set_supported_architectures(&workspace, &["current"]);
    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert!(linked.join("native/host").exists(), "the host's variant is linked again");
    assert!(!linked.join("native/foreign").exists());
    drop((root, npmrc_info));
}
