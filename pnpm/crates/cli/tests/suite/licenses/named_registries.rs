use super::pacquet_in;
use serde_json::{Value, json};
use std::{fs, path::Path};
use tempfile::TempDir;

fn write_licenses_named_registries_fixture() -> TempDir {
    let workspace = TempDir::new().expect("create workspace");
    fs::write(
        workspace.path().join("package.json"),
        json!({
            "name": "named-registries-licenses-app",
            "version": "1.0.0",
            "dependencies": {
                "pkg-a": "^1.0.0",
                "pkg-a-work": "work:pkg-a@^1.0.0",
                "wrapper": "^1.0.0",
                "@scope/pkg": "work:^1.0.0",
                "peer-dep": "^3.0.0",
            },
            "devDependencies": {
                "pkg-peer": "work:^1.5.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.path().join("pnpm-workspace.yaml"),
        "namedRegistries:\n  work: https://npm.work.example.com/\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.path().join("pnpm-lock.yaml"),
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      '@scope/pkg':
        specifier: work:^1.0.0
        version: work:1.0.0
      peer-dep:
        specifier: ^3.0.0
        version: 3.0.0
      pkg-a:
        specifier: ^1.0.0
        version: 1.0.0
      pkg-a-work:
        specifier: work:pkg-a@^1.0.0
        version: pkg-a@work:1.0.0
      wrapper:
        specifier: ^1.0.0
        version: 1.0.0
    devDependencies:
      pkg-peer:
        specifier: work:^1.5.0
        version: work:1.5.0(peer-dep@3.0.0)
packages:
  '@scope/pkg@work:1.0.0':
    resolution: {integrity: sha512-scope-pkg}
  peer-dep@3.0.0:
    resolution: {integrity: sha512-peer-dep}
  pkg-a@1.0.0:
    resolution: {integrity: sha512-pkg-a-public}
  pkg-a@work:1.0.0:
    resolution: {integrity: sha512-pkg-a-work-1}
  pkg-a@work:2.0.0:
    resolution: {integrity: sha512-pkg-a-work-2}
  pkg-peer@work:1.5.0:
    resolution: {integrity: sha512-pkg-peer}
    peerDependencies:
      peer-dep: ^3.0.0
  wrapper@1.0.0:
    resolution: {integrity: sha512-wrapper}
snapshots:
  '@scope/pkg@work:1.0.0': {}
  peer-dep@3.0.0: {}
  pkg-a@1.0.0: {}
  pkg-a@work:1.0.0: {}
  pkg-a@work:2.0.0: {}
  pkg-peer@work:1.5.0(peer-dep@3.0.0):
    dependencies:
      peer-dep: 3.0.0
  wrapper@1.0.0:
    dependencies:
      pkg-a: work:2.0.0
",
    )
    .expect("write pnpm-lock.yaml");

    plant_pkg(
        workspace.path(),
        "pkg-a@1.0.0",
        "pkg-a",
        &json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "license": "MIT",
            "author": "Public Author",
            "description": "Public pkg-a",
        }),
    );
    plant_pkg(
        workspace.path(),
        "pkg-a@work+1.0.0",
        "pkg-a",
        &json!({
            "name": "pkg-a",
            "version": "1.0.0",
            "license": "MIT",
            "author": "Work Author v1",
            "description": "Work pkg-a v1",
        }),
    );
    plant_pkg(
        workspace.path(),
        "pkg-a@work+2.0.0",
        "pkg-a",
        &json!({
            "name": "pkg-a",
            "version": "2.0.0",
            "license": "MIT",
            "author": "Work Author v2",
            "description": "Work pkg-a v2",
        }),
    );
    plant_pkg(
        workspace.path(),
        "@scope+pkg@work+1.0.0",
        "@scope/pkg",
        &json!({ "name": "@scope/pkg", "version": "1.0.0", "license": "Apache-2.0" }),
    );
    plant_pkg(
        workspace.path(),
        "pkg-peer@work+1.5.0_peer-dep@3.0.0",
        "pkg-peer",
        &json!({ "name": "pkg-peer", "version": "1.5.0", "license": "ISC" }),
    );
    plant_pkg(
        workspace.path(),
        "peer-dep@3.0.0",
        "peer-dep",
        &json!({ "name": "peer-dep", "version": "3.0.0", "license": "BSD-2-Clause" }),
    );
    plant_pkg(
        workspace.path(),
        "wrapper@1.0.0",
        "wrapper",
        &json!({ "name": "wrapper", "version": "1.0.0", "license": "BSD-2-Clause" }),
    );

    workspace
}

fn plant_pkg(workspace: &Path, slot: &str, pkg_name: &str, manifest: &Value) {
    let dir = workspace
        .join("node_modules/.pnpm")
        .join(slot)
        .join("node_modules")
        .join(pkg_name);
    fs::create_dir_all(&dir).expect("create virtual store package dir");
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
}

#[test]
fn licenses_list_json_separates_named_registries_and_strips_version_prefixes() {
    let workspace = write_licenses_named_registries_fixture();
    let output = pacquet_in(workspace.path())
        .args(["licenses", "list", "--json"])
        .output()
        .expect("run licenses list --json");
    assert!(
        output.status.success(),
        "licenses list --json failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("parse licenses JSON");
    let mit = report["MIT"].as_array().expect("MIT group");
    assert_eq!(mit.len(), 2, "expected default and work pkg-a to stay separate: {mit:#?}");

    let public_a = &mit[0];
    assert_eq!(public_a["name"], "pkg-a");
    assert_eq!(public_a["versions"], json!(["1.0.0"]));
    assert!(public_a.get("registryName").is_none(), "default registry omits registryName");
    assert_eq!(public_a["author"], "Public Author");

    let work_a = &mit[1];
    assert_eq!(work_a["name"], "pkg-a");
    assert_eq!(work_a["versions"], json!(["1.0.0", "2.0.0"]));
    assert_eq!(work_a["registryName"], "work");
    assert_eq!(work_a["author"], "Work Author v2");

    let apache = &report["Apache-2.0"][0];
    assert_eq!(apache["name"], "@scope/pkg");
    assert_eq!(apache["versions"], json!(["1.0.0"]));
    assert_eq!(apache["registryName"], "work");

    let isc = &report["ISC"][0];
    assert_eq!(isc["name"], "pkg-peer");
    assert_eq!(isc["versions"], json!(["1.5.0"]));
    assert_eq!(isc["registryName"], "work");
}

#[test]
fn licenses_list_table_and_long_display_named_registry_aliases() {
    let workspace = write_licenses_named_registries_fixture();
    let output = pacquet_in(workspace.path())
        .args(["licenses", "list", "--long"])
        .output()
        .expect("run licenses list --long");
    assert!(
        output.status.success(),
        "licenses list --long failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    let table = console::strip_ansi_codes(&String::from_utf8_lossy(&output.stdout)).into_owned();
    assert!(table.contains("│ pkg-a "), "expected bare pkg-a row in:\n{table}");
    assert!(table.contains("pkg-a (work)"), "expected pkg-a (work) row in:\n{table}");
    assert!(table.contains("@scope/pkg (work)"), "expected @scope/pkg (work) row in:\n{table}");
    assert!(
        table.contains("pkg-peer (work) (dev)"),
        "expected pkg-peer (work) (dev) row in:\n{table}",
    );
    assert!(table.contains("Public Author"), "expected Public Author details in:\n{table}");
    assert!(table.contains("Work Author v2"), "expected Work Author v2 details in:\n{table}");
}
