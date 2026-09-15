use super::{copy_fixture, pacquet, parse_sbom_output};
use std::{fs, path::Path};

#[test]
fn sbom_cyclonedx_emits_non_spdx_licenses_as_names() {
    let tmp = copy_fixture("simple-sbom");
    set_root_license(tmp.path(), "BDS-3-Clause");
    write_dependency_manifest(
        tmp.path(),
        &serde_json::json!({
            "name": "is-positive",
            "version": "3.1.0",
            "license": "UNLICENSED",
        }),
    );

    for spec_version in ["1.5", "1.6", "1.7"] {
        let output = pacquet(
            tmp.path(),
            ["sbom", "--sbom-format", "cyclonedx", "--sbom-spec-version", spec_version],
        )
        .output()
        .expect("run pnpm sbom");
        let bom = parse_sbom_output(&output);

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

fn set_root_license(workspace: &Path, license: &str) {
    let manifest_path = workspace.join("package.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read the root manifest"))
            .expect("parse the root manifest");
    manifest["license"] = serde_json::Value::String(license.to_string());
    fs::write(manifest_path, manifest.to_string()).expect("write the root manifest");
}

fn write_dependency_manifest(workspace: &Path, manifest: &serde_json::Value) {
    let package_dir =
        workspace.join("node_modules/.pnpm/is-positive@3.1.0/node_modules/is-positive");
    fs::create_dir_all(&package_dir).expect("create the package directory");
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
