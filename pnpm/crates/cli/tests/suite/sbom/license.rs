use super::{copy_fixture, pacquet, parse_sbom_output};
use std::{fs, path::Path};

const SPEC_VERSIONS: [&str; 3] = ["1.5", "1.6", "1.7"];

#[test]
fn sbom_cyclonedx_emits_non_spdx_licenses_as_names() {
    let tmp = copy_fixture("simple-sbom");
    set_root_license(tmp.path(), "BDS-3-Clause");
    set_dependency_license(tmp.path(), "UNLICENSED");

    for spec_version in SPEC_VERSIONS {
        let bom = cyclonedx_bom(tmp.path(), spec_version);

        assert_eq!(
            bom["metadata"]["component"]["licenses"],
            serde_json::json!([{ "license": { "name": "BDS-3-Clause" } }]),
        );
        assert_eq!(
            cyclonedx_component(&bom, "is-positive")["licenses"],
            serde_json::json!([{ "license": { "name": "UNLICENSED" } }]),
        );
    }
}

#[test]
fn sbom_cyclonedx_emits_spdx_identifiers_and_expressions() {
    let tmp = copy_fixture("simple-sbom");
    set_root_license(tmp.path(), "(MIT OR Apache-2.0)");
    set_dependency_license(tmp.path(), "mit");

    for spec_version in SPEC_VERSIONS {
        let bom = cyclonedx_bom(tmp.path(), spec_version);

        assert_eq!(
            bom["metadata"]["component"]["licenses"],
            serde_json::json!([{ "expression": "(MIT OR Apache-2.0)" }]),
        );
        assert_eq!(
            cyclonedx_component(&bom, "is-positive")["licenses"],
            serde_json::json!([{ "license": { "id": "MIT" } }]),
        );
    }
}

fn cyclonedx_bom(workspace: &Path, spec_version: &str) -> serde_json::Value {
    let output = pacquet(
        workspace,
        ["sbom", "--sbom-format", "cyclonedx", "--sbom-spec-version", spec_version],
    )
    .output()
    .expect("run pnpm sbom");
    parse_sbom_output(&output)
}

fn set_root_license(workspace: &Path, license: &str) {
    let manifest_path = workspace.join("package.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read the root manifest"))
            .expect("parse the root manifest");
    manifest["license"] = serde_json::Value::String(license.to_string());
    fs::write(manifest_path, manifest.to_string()).expect("write the root manifest");
}

fn set_dependency_license(workspace: &Path, license: &str) {
    let package_dir =
        workspace.join("node_modules/.pnpm/is-positive@3.1.0/node_modules/is-positive");
    fs::create_dir_all(&package_dir).expect("create the package directory");
    let manifest = serde_json::json!({
        "name": "is-positive",
        "version": "3.1.0",
        "license": license,
    });
    fs::write(package_dir.join("package.json"), manifest.to_string())
        .expect("write the package manifest");
}

fn cyclonedx_component<'a>(bom: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    bom["components"]
        .as_array()
        .expect("components array")
        .iter()
        .find(|component| component["name"] == name)
        .unwrap_or_else(|| panic!("find the {name} component"))
}
