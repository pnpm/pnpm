use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use std::{ffi::OsStr, fs, path::Path, process::Command};
use tempfile::TempDir;

fn copy_fixture(name: &str) -> TempDir {
    let tmp = TempDir::new().expect("create temp dir");
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../pnpm11/deps/compliance/commands/test/sbom/fixtures")
        .join(name);
    for entry in fs::read_dir(&fixture_dir).expect("read fixture dir") {
        let entry = entry.expect("read dir entry");
        let dest = tmp.path().join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_dir_recursive(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).expect("copy file");
        }
    }
    tmp
}

fn copy_dir_recursive(src: &Path, dest: &Path) {
    fs::create_dir_all(dest).expect("create dir");
    for entry in fs::read_dir(src).expect("read dir") {
        let entry = entry.expect("read entry");
        let target = dest.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_dir_recursive(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy file");
        }
    }
}

fn pacquet(workspace: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(workspace)
        .with_args(args)
}

fn run_sbom_json(workspace: &Path, format: &str, extra_args: &[&str]) -> serde_json::Value {
    let mut args = vec!["sbom", "--sbom-format", format, "--lockfile-only"];
    args.extend_from_slice(extra_args);
    let output = pacquet(workspace, args).output().expect("run pacquet");
    assert!(
        output.status.success(),
        "pacquet sbom failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("parse JSON output")
}

#[test]
fn sbom_cyclonedx_basic() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);

    assert_eq!(parsed["bomFormat"], "CycloneDX");
    assert_eq!(parsed["specVersion"], "1.7");
    assert_eq!(parsed["metadata"]["component"]["name"], "simple-sbom-test");
    assert_eq!(parsed["metadata"]["component"]["version"], "1.0.0");

    let components = parsed["components"].as_array().expect("components array");
    assert!(!components.is_empty());

    let is_positive =
        components.iter().find(|c| c["name"] == "is-positive").expect("find is-positive");
    assert_eq!(is_positive["purl"], "pkg:npm/is-positive@3.1.0");
    assert_eq!(is_positive["version"], "3.1.0");
}

#[test]
fn sbom_spdx_basic() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);

    assert_eq!(parsed["spdxVersion"], "SPDX-2.3");
    assert_eq!(parsed["dataLicense"], "CC0-1.0");

    let packages = parsed["packages"].as_array().expect("packages array");
    assert!(packages.len() > 1);

    let root = &packages[0];
    assert_eq!(root["name"], "simple-sbom-test");
    assert_eq!(root["versionInfo"], "1.0.0");
}

#[test]
fn sbom_missing_format_fails() {
    let tmp = copy_fixture("simple-sbom");
    let output = pacquet(tmp.path(), ["sbom"]).output().expect("run pacquet");
    assert!(!output.status.success());
}

#[test]
fn sbom_prod_excludes_dev() {
    let tmp = copy_fixture("with-dev-dependency");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--prod"]);

    let components = parsed["components"].as_array().expect("components array");
    assert!(components.iter().any(|c| c["name"] == "is-positive"), "prod dep should be included");
    assert!(
        !components.iter().any(|c| c["name"] == "typescript"),
        "dev dep should be excluded with --prod",
    );
}

#[test]
fn sbom_dev_only_scope_excluded() {
    let tmp = copy_fixture("with-dev-dependency");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);

    let components = parsed["components"].as_array().expect("components array");
    let typescript =
        components.iter().find(|c| c["name"] == "typescript").expect("find typescript");
    assert_eq!(typescript["scope"], "excluded");

    let props = typescript["properties"].as_array().expect("properties");
    assert!(
        props.iter().any(|p| p["name"] == "cdx:npm:package:development" && p["value"] == "true")
    );
}

#[test]
fn sbom_spec_version_1_6() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--sbom-spec-version", "1.6"]);
    assert_eq!(parsed["specVersion"], "1.6");
    assert!(parsed["$schema"].as_str().unwrap().contains("1.6"));
}

#[test]
fn sbom_invalid_spec_version_fails() {
    let tmp = copy_fixture("simple-sbom");
    let output = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--sbom-spec-version", "2.0"],
    )
    .output()
    .expect("run pacquet");
    assert!(!output.status.success());
}

#[test]
fn sbom_spec_version_with_spdx_fails() {
    let tmp = copy_fixture("simple-sbom");
    let output = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "spdx", "--lockfile-only", "--sbom-spec-version", "1.6"],
    )
    .output()
    .expect("run pacquet");
    assert!(!output.status.success());
}

#[test]
fn sbom_application_type() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--sbom-type", "application"]);
    assert_eq!(parsed["metadata"]["component"]["type"], "application");
}

#[test]
fn sbom_has_serial_number() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let serial = parsed["serialNumber"].as_str().expect("serialNumber");
    assert!(serial.starts_with("urn:uuid:"), "serialNumber should start with urn:uuid:");
}

#[test]
fn sbom_has_timestamp() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    assert!(parsed["metadata"]["timestamp"].is_string());
}

#[test]
fn sbom_has_tools() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let tools = parsed["metadata"]["tools"]["components"].as_array().expect("tools");
    assert!(tools.iter().any(|t| t["name"] == "pacquet"));
}

#[test]
fn sbom_dependencies_present() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let deps = parsed["dependencies"].as_array().expect("dependencies array");
    assert!(!deps.is_empty());

    let root_dep = deps
        .iter()
        .find(|d| d["ref"].as_str().unwrap().contains("simple-sbom-test"))
        .expect("root in dependencies");
    let depends_on = root_dep["dependsOn"].as_array().expect("dependsOn");
    assert!(depends_on.iter().any(|d| d.as_str().unwrap().contains("is-positive")));
}

#[test]
fn sbom_root_license() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let licenses = parsed["metadata"]["component"]["licenses"].as_array().expect("licenses");
    assert!(!licenses.is_empty());
}

#[test]
fn sbom_root_description() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    assert!(parsed["metadata"]["component"]["description"].is_string());
}

#[test]
fn sbom_spdx_creation_info() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);
    assert!(parsed["creationInfo"]["created"].is_string());
    let creators = parsed["creationInfo"]["creators"].as_array().expect("creators");
    assert!(creators.iter().any(|c| c.as_str().unwrap().contains("pacquet")));
}

#[test]
fn sbom_spdx_describes_relationship() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);
    let rels = parsed["relationships"].as_array().expect("relationships");
    assert!(rels.iter().any(|r| r["relationshipType"] == "DESCRIBES"));
}

#[test]
fn sbom_component_has_distribution_ref() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let components = parsed["components"].as_array().expect("components");
    let is_positive = components.iter().find(|c| c["name"] == "is-positive").expect("is-positive");
    let ext_refs = is_positive["externalReferences"].as_array().expect("externalReferences");
    assert!(ext_refs.iter().any(|r| r["type"] == "distribution"));
}

#[test]
fn sbom_out_writes_file() {
    let tmp = copy_fixture("simple-sbom");
    let out_path = tmp.path().join("sbom.json");
    let output = pacquet(
        tmp.path(),
        [
            "sbom",
            "--sbom-format",
            "cyclonedx",
            "--lockfile-only",
            "--out",
            out_path.to_str().unwrap(),
        ],
    )
    .output()
    .expect("run pacquet");
    assert!(output.status.success());
    assert!(out_path.exists(), "output file should be created");
    let content: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out_path).unwrap()).expect("valid JSON");
    assert_eq!(content["bomFormat"], "CycloneDX");
}

#[test]
fn sbom_exclude_peers() {
    let tmp = copy_fixture("with-peer-dependency");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--exclude-peers"]);
    let components = parsed["components"].as_array().expect("components");
    assert!(components.iter().any(|c| c["name"] == "is-positive"), "non-peer dep should remain");
}
