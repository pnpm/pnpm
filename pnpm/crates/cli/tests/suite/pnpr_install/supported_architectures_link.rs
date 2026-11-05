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

/// The hoisted linker's package directory does not name the archive either,
/// so switching the platform imports the other variant though the lockfile is
/// unchanged.
#[test]
fn the_hoisted_linker_relinks_when_the_platform_changes() {
    let (registry, token) = start_hosted_pnpr();
    publish_cli(&registry, &token);
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = consumer(&registry, &token);
    let manifest = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&manifest).expect("read pnpm-workspace.yaml");
    writeln!(yaml, "nodeLinker: hoisted").expect("write nodeLinker");
    fs::write(&manifest, yaml).expect("write pnpm-workspace.yaml");
    set_supported_architectures(&workspace, &[foreign_target()]);

    pacquet
        .with_arg("install")
        .assert()
        .success();
    let linked = workspace.join("node_modules/@acme/cli");
    assert!(linked.join("native/foreign").exists(), "the listed platform's variant is linked");

    set_supported_architectures(&workspace, &["current"]);
    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert!(linked.join("native/host").exists(), "the host's variant is linked again");
    assert!(!linked.join("native/foreign").exists());
    drop((root, npmrc_info));
}

/// The content of every file in `store_dir` that a published archive of
/// [`publish_cli`] holds.
fn stored_files(store_dir: &Path) -> Vec<String> {
    let mut files: Vec<String> = pnpm_testing_utils::fs::get_all_files(store_dir)
        .into_iter()
        .filter_map(|path| fs::read_to_string(store_dir.join(path)).ok())
        .filter(|content| {
            ["portable/any", "native/host", "native/foreign", "dep.js"].contains(&content.as_str())
        })
        .collect();
    files.sort();
    files.dedup();
    files
}

/// Empty `store_dir` and `workspace`'s `node_modules`, leaving the lockfile:
/// resolving prefetches every package the lockfile names, so only an
/// install from the lockfile shows what the install itself stores.
fn forget_installed(workspace: &Path, store_dir: &Path) {
    for dir in [store_dir, &workspace.join("node_modules")] {
        if dir.exists() {
            fs::remove_dir_all(dir).expect("remove directory");
        }
    }
}

fn frozen_install(workspace: &Path) {
    pacquet_at(workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
}

#[test]
fn another_listed_platforms_variant_and_its_dependency_are_stored_not_linked() {
    let (registry, token) = start_hosted_pnpr();
    publish_cli(&registry, &token);
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = consumer(&registry, &token);
    set_supported_architectures(&workspace, &["current", foreign_target()]);
    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    forget_installed(&workspace, &npmrc_info.store_dir);

    frozen_install(&workspace);
    assert_eq!(stored_files(&npmrc_info.store_dir), ["dep.js", "native/foreign", "native/host"]);
    let linked = workspace.join("node_modules/@acme/cli");
    assert!(linked.join("native/host").exists(), "the host's variant is linked");
    assert!(!workspace.join("node_modules/.pnpm/node_modules/@acme/dep").exists());
    assert!(!linked.join("node_modules/@acme/dep").exists());
    drop((root, npmrc_info));
}

#[test]
fn a_platform_listed_after_a_warm_install_is_stored_with_its_dependency() {
    let (registry, token) = start_hosted_pnpr();
    publish_cli(&registry, &token);
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = consumer(&registry, &token);
    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    forget_installed(&workspace, &npmrc_info.store_dir);

    frozen_install(&workspace);
    frozen_install(&workspace);
    assert_eq!(
        stored_files(&npmrc_info.store_dir),
        ["native/host"],
        "without supportedArchitectures only the linked archive is stored",
    );

    set_supported_architectures(&workspace, &["current", foreign_target()]);
    frozen_install(&workspace);
    assert_eq!(stored_files(&npmrc_info.store_dir), ["dep.js", "native/foreign", "native/host"]);
    drop((root, npmrc_info));
}

#[test]
fn fetch_stores_the_archives_of_every_listed_platform_and_their_dependencies() {
    let (registry, token) = start_hosted_pnpr();
    publish_cli(&registry, &token);
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = consumer(&registry, &token);
    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    forget_installed(&workspace, &npmrc_info.store_dir);

    set_supported_architectures(&workspace, &["current", foreign_target()]);
    pacquet_at(&workspace)
        .with_arg("fetch")
        .assert()
        .success();
    assert_eq!(stored_files(&npmrc_info.store_dir), ["dep.js", "native/foreign", "native/host"]);
    drop((root, npmrc_info));
}

/// The walk starts at what the install materializes, so a production
/// install keeps no archive for a platform of a dev dependency.
#[test]
fn a_production_install_stores_nothing_for_a_dev_dependency() {
    let (registry, token) = start_hosted_pnpr();
    publish_cli(&registry, &token);
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = consumer(&registry, &token);
    fs::write(
        workspace.join("package.json"),
        json!({ "devDependencies": { "@acme/cli": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    set_supported_architectures(&workspace, &["current", foreign_target()]);
    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    forget_installed(&workspace, &npmrc_info.store_dir);

    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile", "--prod"])
        .assert()
        .success();
    assert_eq!(stored_files(&npmrc_info.store_dir), Vec::<String>::new());
    drop((root, npmrc_info));
}
