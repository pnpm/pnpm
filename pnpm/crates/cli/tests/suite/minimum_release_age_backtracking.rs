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
}

#[test]
fn install_add_and_latest_update_back_off_from_immature_exact_pins() {
    assert_backoff(&Fixture { latest_major: 2, ..Default::default() });
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
    let selector = if fixture.named_parent { "gh:*" } else { "*" };
    let manifest = if command == "add" {
        json!({ "name": "test-project", "version": "1.0.0" })
    } else {
        json!({ "name": "test-project", "version": "1.0.0", "dependencies": { "parent": selector } })
    };
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
    pacquet
        .args(["--lockfile-only", "--ignore-scripts"])
        .assert()
        .success();
    assert_resolution(fixture, command, &workspace);
    drop(root);
}

fn parent_packument(fixture: &Fixture) -> serde_json::Value {
    let mut versions = serde_json::Map::new();
    let mut time = serde_json::Map::new();
    for major in 1..=fixture.latest_major {
        let version = format!("{major}.0.0");
        let child = if major == 1 { "1.0.0" } else { "2.0.0" };
        let mut manifest = version_manifest("parent", &version);
        let group = if fixture.optional { "optionalDependencies" } else { "dependencies" };
        manifest[group] = json!({ "child": format!("{}{child}", fixture.pin_prefix) });
        versions.insert(version.clone(), manifest);
        time.insert(version, json!("2020-01-01T00:00:00Z"));
    }
    json!({
        "name": "parent", "dist-tags": { "latest": format!("{}.0.0", fixture.latest_major) },
        "versions": versions, "time": time,
    })
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
    let parent_prefix = if fixture.named_parent { "gh:" } else { "" };
    assert!(lockfile.contains(&format!("parent@{parent_prefix}1.0.0:")), "{command}: {lockfile}");
    assert!(!lockfile.contains(&format!("parent@{parent_prefix}2.0.0:")), "{command}: {lockfile}");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(workspace.join("package.json")).unwrap()).unwrap();
    if command == "add" && !fixture.named_parent {
        assert_eq!(manifest["dependencies"]["parent"], "^1.0.0");
    }
}
