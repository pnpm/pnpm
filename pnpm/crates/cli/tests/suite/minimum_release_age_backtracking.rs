use assert_cmd::prelude::*;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::fs;

#[derive(Default)]
struct Fixture {
    latest_major: usize,
    pin_prefix: &'static str,
    optional: bool,
    named_parent: bool,
    mismatched_parent: bool,
    mismatched_version: bool,
    raw_parent_key: Option<&'static str>,
}

#[test]
fn install_add_and_latest_update_back_off_from_immature_exact_pins() {
    assert_backoff(&Fixture { latest_major: 2, ..Default::default() });
}

#[test]
fn colliding_manifest_names_do_not_merge_requested_parent_identities() {
    assert_backoff(&Fixture { latest_major: 2, mismatched_parent: true, ..Default::default() });
}

#[test]
fn backoff_blocks_packument_keys_that_differ_from_manifest_versions() {
    assert_backoff(&Fixture {
        latest_major: 2,
        mismatched_parent: true,
        mismatched_version: true,
        ..Default::default()
    });
}

#[test]
fn raw_parent_keys_reach_maturity_preflight() {
    for raw_parent_key in ["v2.0.0", "banana"] {
        assert_backoff(&Fixture {
            latest_major: 2,
            raw_parent_key: Some(raw_parent_key),
            ..Default::default()
        });
    }
}

#[test]
fn add_reaches_an_installable_major_beyond_nine_rejected_candidates() {
    assert_backoff(&Fixture { latest_major: 12, ..Default::default() });
}

#[test]
fn named_registry_update_checks_the_selected_manifest() {
    assert_backoff(&Fixture { latest_major: 2, named_parent: true, ..Default::default() });
}

#[test]
fn named_and_prefixed_exact_pins_back_off_in_both_dependency_groups() {
    for pin_prefix in ["gh:", "v", "V"] {
        for optional in [false, true] {
            assert_backoff(&Fixture {
                latest_major: 2,
                pin_prefix,
                optional,
                ..Default::default()
            });
        }
    }
}

fn assert_backoff(fixture: &Fixture) {
    let mut default_registry = mockito::Server::new();
    let mut named_registry = mockito::Server::new();
    let selected = if fixture.named_parent { &mut named_registry } else { &mut default_registry };
    let _parent = selected
        .mock("GET", "/parent")
        .with_status(200)
        .with_body(parent_packument(fixture).to_string())
        .create();
    let _same_manifest = default_registry
        .mock("GET", "/decoy")
        .with_status(200)
        .with_body(
            json!({
                "name": "decoy", "dist-tags": { "latest": "2.0.0" },
                "versions": { "2.0.0": version_manifest("other", "2.0.0") },
                "time": { "2.0.0": "2020-01-01T00:00:00Z" },
            })
            .to_string(),
        )
        .create();
    let _default_child = default_registry
        .mock("GET", "/child")
        .with_status(200)
        .with_body(child_packument().to_string())
        .create();
    let _named_child = named_registry
        .mock("GET", "/child")
        .with_status(200)
        .with_body(child_packument().to_string())
        .create();
    let decoy = fixture.named_parent.then(|| {
        default_registry
            .mock("GET", "/parent")
            .with_status(200)
            .with_body(
                parent_packument(&Fixture { latest_major: 1, ..Default::default() }).to_string(),
            )
            .expect(0)
            .create()
    });
    for command in ["install", "add", "update"] {
        run_command(fixture, command, &default_registry.url(), &named_registry.url());
    }
    if let Some(decoy) = decoy {
        decoy.assert();
    }
}

fn run_command(fixture: &Fixture, command: &str, default_url: &str, named_url: &str) {
    let CommandTempCwd { mut pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = project_manifest(fixture, command);
    fs::write(workspace.join("package.json"), manifest.to_string()).unwrap();
    fs::write(workspace.join(".npmrc"), format!("registry={default_url}/\n")).unwrap();
    fs::write(workspace.join("pnpm-workspace.yaml"), format!(
        "minimumReleaseAge: 1440\nminimumReleaseAgeStrict: true\nstoreDir: ../store\ncacheDir: ../cache\nenableGlobalVirtualStore: false\nregistries:\n  '{named_url}/':\n    prefix: gh\n",
    )).unwrap();
    pacquet.arg(command);
    match command {
        "add" => {
            pacquet.arg(if fixture.named_parent { "parent@gh:*" } else { "parent" });
        }
        "update" => {
            pacquet.args(["parent", "--latest"]);
        }
        _ => {}
    }
    let result = pacquet
        .args(["--lockfile-only", "--ignore-scripts"])
        .assert()
        .success();
    if fixture.named_parent && command == "install" {
        assert!(
            String::from_utf8_lossy(&result.get_output().stdout)
                .contains("resolved to gh:1.0.0 instead"),
        );
    }
    assert_resolution(fixture, command, &workspace);
    drop(root);
}

fn project_manifest(fixture: &Fixture, command: &str) -> serde_json::Value {
    let selector = if fixture.named_parent { "gh:*" } else { "*" };
    let mut manifest = if command == "add" {
        json!({ "name": "test-project", "version": "1.0.0" })
    } else {
        json!({ "name": "test-project", "version": "1.0.0", "dependencies": { "parent": selector } })
    };
    if fixture.mismatched_parent {
        manifest["dependencies"]["decoy"] = json!("2.0.0");
    }
    manifest
}

fn parent_packument(fixture: &Fixture) -> serde_json::Value {
    let mut versions = serde_json::Map::new();
    let mut time = serde_json::Map::new();
    for major in 1..=fixture.latest_major {
        let version = parent_version_key(fixture, major);
        versions.insert(version.clone(), parent_manifest(fixture, major));
        time.insert(version, json!("2020-01-01T00:00:00Z"));
    }
    json!({
        "name": "parent", "dist-tags": { "latest": parent_version_key(fixture, fixture.latest_major) },
        "versions": versions, "time": time,
    })
}

fn parent_version_key(fixture: &Fixture, major: usize) -> String {
    if major > 1
        && let Some(raw_key) = fixture.raw_parent_key
    {
        return raw_key.to_string();
    }
    let suffix = if fixture.mismatched_version && major > 1 { "+build" } else { "" };
    format!("{major}.0.0{suffix}")
}

fn parent_manifest(fixture: &Fixture, major: usize) -> serde_json::Value {
    let child = if major == 1 { "1.0.0" } else { "2.0.0" };
    let name = if fixture.mismatched_parent && major > 1 { "other" } else { "parent" };
    let mut manifest = version_manifest(name, &format!("{major}.0.0"));
    let group = if fixture.optional { "optionalDependencies" } else { "dependencies" };
    manifest[group] = json!({ "child": format!("{}{child}", fixture.pin_prefix) });
    manifest
}

fn child_packument() -> serde_json::Value {
    json!({
        "name": "child", "dist-tags": { "latest": "2.0.0" },
        "versions": { "1.0.0": version_manifest("child", "1.0.0"), "2.0.0": version_manifest("child", "2.0.0") },
        "time": { "1.0.0": "2020-01-01T00:00:00Z", "2.0.0": "2099-01-01T00:00:00Z" },
    })
}

fn version_manifest(name: &str, version: &str) -> serde_json::Value {
    json!({
        "name": name, "version": version,
        "dist": {
            "tarball": format!("https://registry.example/{name}-{version}.tgz"),
            "integrity": ssri::Integrity::from(b"fixture".as_slice()).to_string(),
        },
    })
}

fn assert_resolution(fixture: &Fixture, command: &str, workspace: &std::path::Path) {
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
    if fixture.mismatched_parent {
        assert!(lockfile.contains("decoy@2.0.0:"), "{command}: {lockfile}");
    }
    let parent_prefix = if fixture.named_parent { "gh:" } else { "" };
    assert!(lockfile.contains(&format!("parent@{parent_prefix}1.0.0:")), "{command}: {lockfile}");
    assert!(!lockfile.contains(&format!("parent@{parent_prefix}2.0.0:")), "{command}: {lockfile}");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(workspace.join("package.json")).unwrap()).unwrap();
    if command == "add" && !fixture.named_parent {
        assert_eq!(manifest["dependencies"]["parent"], "^1.0.0");
    }
}

#[test]
fn frozen_install_preserves_tarball_url_when_manifest_name_differs() {
    use pnpm_testing_utils::{command_env::CommandTestExt, fixtures::minimal_tarball};
    use std::process::Command;

    let mut server = mockito::Server::new();
    let tarball_url = format!("{}/other/-/other-1.0.0.tgz", server.url());
    let archive = minimal_tarball("other", "1.0.0");
    let integrity = ssri::Integrity::from(archive.as_slice()).to_string();
    let _metadata = server
        .mock("GET", "/requested")
        .with_status(200)
        .with_body(
            json!({
                "name": "other",
                "dist-tags": { "latest": "1.0.0" },
                "versions": { "1.0.0": {
                    "name": "other", "version": "1.0.0", "dist": { "tarball": tarball_url, "integrity": integrity }
                }}
            })
            .to_string(),
        )
        .create();
    let tarball = server
        .mock("GET", "/other/-/other-1.0.0.tgz")
        .with_status(200)
        .with_body(archive)
        .expect(1)
        .create();
    let CommandTempCwd { root: _root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({ "dependencies": { "requested": "1.0.0" } }).to_string(),
    )
    .unwrap();
    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", server.url())).unwrap();
    fs::write(workspace.join("pnpm-workspace.yaml"), "storeDir: ../store\ncacheDir: ../cache\nenableGlobalVirtualStore: false\nminimumReleaseAge: 0\nstrictStorePkgContentCheck: false\n").unwrap();
    for mode in ["--lockfile-only", "--frozen-lockfile"] {
        Command::cargo_bin("pnpm")
            .unwrap()
            .without_ambient_pnpm_config()
            .current_dir(&workspace)
            .args(["install", mode, "--ignore-scripts"])
            .assert()
            .success();
        let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
        assert!(lockfile.contains(&tarball_url));
        assert!(lockfile.contains(&integrity));
    }
    tarball.assert();
    let installed: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("node_modules/requested/package.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(installed["name"], "other");
}
