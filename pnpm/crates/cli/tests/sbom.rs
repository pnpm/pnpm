use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use std::{ffi::OsStr, fs, path::Path, process::Command};
use tempfile::TempDir;

fn copy_fixture(name: &str) -> TempDir {
    let tmp = TempDir::new().expect("create temp dir");
    let local_fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    let fixture_dir = if local_fixture.exists() {
        local_fixture
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../pnpm11/deps/compliance/commands/test/sbom/fixtures")
            .join(name)
    };
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
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
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
        String::from_utf8_lossy(&output.stderr),
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
        components.iter().find(|comp| comp["name"] == "is-positive").expect("find is-positive");
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
    assert!(
        components.iter().any(|comp| comp["name"] == "is-positive"),
        "prod dep should be included",
    );
    assert!(
        !components.iter().any(|comp| comp["name"] == "typescript"),
        "dev dep should be excluded with --prod",
    );
}

#[test]
fn sbom_dev_only_scope_excluded() {
    let tmp = copy_fixture("with-dev-dependency");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);

    let components = parsed["components"].as_array().expect("components array");
    let typescript =
        components.iter().find(|comp| comp["name"] == "typescript").expect("find typescript");
    assert_eq!(typescript["scope"], "excluded");

    let props = typescript["properties"].as_array().expect("properties");
    assert!(
        props
            .iter()
            .any(|prop| prop["name"] == "cdx:npm:package:development" && prop["value"] == "true"),
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
    assert!(tools.iter().any(|tool| tool["name"] == "pnpm"));
}

#[test]
fn sbom_dependencies_present() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let deps = parsed["dependencies"].as_array().expect("dependencies array");
    assert!(!deps.is_empty());

    let root_dep = deps
        .iter()
        .find(|dep| dep["ref"].as_str().unwrap().contains("simple-sbom-test"))
        .expect("root in dependencies");
    let depends_on = root_dep["dependsOn"].as_array().expect("dependsOn");
    assert!(depends_on.iter().any(|dep| dep.as_str().unwrap().contains("is-positive")));
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
    assert!(creators.iter().any(|creator| creator.as_str().unwrap().contains("pnpm")));
}

#[test]
fn sbom_spdx_describes_relationship() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);
    let rels = parsed["relationships"].as_array().expect("relationships");
    assert!(rels.iter().any(|rel| rel["relationshipType"] == "DESCRIBES"));
}

#[test]
fn sbom_component_has_distribution_ref() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let components = parsed["components"].as_array().expect("components");
    let is_positive =
        components.iter().find(|comp| comp["name"] == "is-positive").expect("is-positive");
    let ext_refs = is_positive["externalReferences"].as_array().expect("externalReferences");
    assert!(ext_refs.iter().any(|ext_ref| ext_ref["type"] == "distribution"));
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
fn sbom_includes_peers_by_default() {
    let tmp = copy_fixture("with-peer-dependency");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let components = parsed["components"].as_array().expect("components");
    assert!(components.iter().any(|comp| comp["name"] == "is-positive"));
    assert!(
        components.iter().any(|comp| comp["name"] == "is-odd"),
        "peer dep should be included by default",
    );
    assert!(
        components.iter().any(|comp| comp["name"] == "is-number"),
        "transitive of peer should be included",
    );
}

#[test]
fn sbom_exclude_peers_drops_subtree() {
    let tmp = copy_fixture("with-peer-dependency");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--exclude-peers"]);
    let components = parsed["components"].as_array().expect("components");
    assert!(
        components.iter().any(|comp| comp["name"] == "is-positive"),
        "non-peer dep should remain",
    );
    assert!(!components.iter().any(|comp| comp["name"] == "is-odd"), "peer dep should be excluded");
    assert!(
        !components.iter().any(|comp| comp["name"] == "is-number"),
        "transitive dep reachable only through peer should be excluded",
    );
    let root_ref = parsed["metadata"]["component"]["bom-ref"].as_str().expect("bom-ref");
    let root_deps = parsed["dependencies"]
        .as_array()
        .expect("deps")
        .iter()
        .find(|dep| dep["ref"] == root_ref)
        .expect("root deps");
    assert!(
        !root_deps["dependsOn"]
            .as_array()
            .expect("dependsOn")
            .iter()
            .any(|dep| dep.as_str().unwrap().contains("is-odd")),
        "peer should not appear in root dependency graph",
    );
}

#[test]
fn sbom_exclude_peers_workspace_sub_packages() {
    let tmp = copy_fixture("with-peer-workspace");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--exclude-peers"]);
    let components = parsed["components"].as_array().expect("components");
    assert!(components.iter().any(|comp| comp["name"] == "is-positive"));
    assert!(
        !components.iter().any(|comp| comp["name"] == "is-odd"),
        "peer in sub-package should be excluded",
    );
}

#[test]
fn sbom_exclude_peers_tolerates_malformed_manifest() {
    let tmp = copy_fixture("with-peer-workspace");
    fs::write(tmp.path().join("packages/pkg-a/package.json"), "{ not valid json").unwrap();
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--exclude-peers"]);
    let components = parsed["components"].as_array().expect("components");
    assert!(
        components.iter().any(|comp| comp["name"] == "is-positive"),
        "should still produce output",
    );
}

#[test]
fn sbom_exclude_peers_keeps_real_dep_in_other_importer() {
    let tmp = copy_fixture("with-peer-and-real-dep");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--exclude-peers"]);
    let components = parsed["components"].as_array().expect("components");
    assert!(
        components.iter().any(|comp| comp["name"] == "is-odd"),
        "is-odd is a peer in pkg-a but a real dep in pkg-b; should be kept",
    );
}

#[test]
fn sbom_out_interpolates_percent_s() {
    let tmp = copy_fixture("simple-sbom");
    let out_pattern = tmp.path().join("sbom-out/%s.cdx.json");
    let output = pacquet(
        tmp.path(),
        [
            "sbom",
            "--sbom-format",
            "cyclonedx",
            "--lockfile-only",
            "--out",
            out_pattern.to_str().unwrap(),
        ],
    )
    .output()
    .expect("run pacquet");
    assert!(output.status.success());
    let expected = tmp.path().join("sbom-out/simple-sbom-test.cdx.json");
    assert!(expected.exists(), "interpolated %s file should exist");
}

#[test]
fn sbom_out_interpolates_percent_v() {
    let tmp = copy_fixture("simple-sbom");
    let out_pattern = tmp.path().join("sbom-out/%s-%v.cdx.json");
    let output = pacquet(
        tmp.path(),
        [
            "sbom",
            "--sbom-format",
            "cyclonedx",
            "--lockfile-only",
            "--out",
            out_pattern.to_str().unwrap(),
        ],
    )
    .output()
    .expect("run pacquet");
    assert!(output.status.success());
    let expected = tmp.path().join("sbom-out/simple-sbom-test-1.0.0.cdx.json");
    assert!(expected.exists(), "interpolated %s-%v file should exist");
}

#[test]
fn sbom_dev_flag_excludes_prod() {
    let tmp = copy_fixture("with-dev-dependency");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--dev"]);
    let components = parsed["components"].as_array().expect("components");
    assert!(
        !components.iter().any(|comp| comp["name"] == "is-positive"),
        "prod dep should be excluded with --dev",
    );
    assert!(
        components.iter().any(|comp| comp["name"] == "typescript"),
        "dev dep should be included",
    );
}

#[test]
fn sbom_split_outputs_ndjson() {
    let tmp = copy_fixture("workspace-sbom");
    let output =
        pacquet(tmp.path(), ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--split"])
            .output()
            .expect("run pacquet");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|line| !line.is_empty()).collect();
    // Fixture lockfile only has root importer (TS tests install first to populate all importers)
    assert!(!lines.is_empty(), "should output at least one NDJSON line");
    for line in &lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("each line should be valid JSON");
        assert_eq!(parsed["bomFormat"], "CycloneDX");
    }
}

#[test]
fn sbom_split_out_writes_per_package_files() {
    let tmp = copy_fixture("workspace-sbom");
    let out_pattern = tmp.path().join("sbom-out/%s.cdx.json");
    let output = pacquet(
        tmp.path(),
        [
            "sbom",
            "--sbom-format",
            "cyclonedx",
            "--lockfile-only",
            "--split",
            "--out",
            out_pattern.to_str().unwrap(),
        ],
    )
    .output()
    .expect("run pacquet");
    assert!(output.status.success());
    let out_dir = tmp.path().join("sbom-out");
    assert!(out_dir.exists(), "output directory should be created");
    let files: Vec<String> = fs::read_dir(&out_dir)
        .expect("read output dir")
        .filter_map(|entry| entry.ok().map(|entry| entry.file_name().to_string_lossy().to_string()))
        .collect();
    assert!(!files.is_empty(), "should write at least one file");
}

#[test]
fn sbom_split_out_without_percent_s_fails() {
    let tmp = copy_fixture("workspace-sbom");
    let output = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--split", "--out", "sbom.json"],
    )
    .output()
    .expect("run pacquet");
    assert!(!output.status.success(), "--split --out without %s should fail");
}

#[test]
fn sbom_spdx_license_from_manifest() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);
    let root = &parsed["packages"].as_array().expect("packages")[0];
    assert_eq!(root["licenseConcluded"], "ISC");
    assert_eq!(root["licenseDeclared"], "ISC");
}

#[test]
fn sbom_lifecycle_pre_build_in_lockfile_only() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let phase = parsed["metadata"]["lifecycles"][0]["phase"].as_str().expect("phase");
    assert_eq!(phase, "pre-build");
}

#[test]
fn sbom_spdx_download_location() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);
    let packages = parsed["packages"].as_array().expect("packages");
    let is_positive =
        packages.iter().find(|pkg| pkg["name"] == "is-positive").expect("is-positive");
    let dl = is_positive["downloadLocation"].as_str().expect("downloadLocation");
    assert!(dl.contains("registry.npmjs.org"), "should have registry URL, got {dl}");
}

#[test]
fn sbom_authors_in_metadata() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--sbom-authors", "Alice, Bob"]);
    let authors = parsed["metadata"]["authors"].as_array().expect("authors");
    assert_eq!(authors.len(), 2);
    assert_eq!(authors[0]["name"], "Alice");
    assert_eq!(authors[1]["name"], "Bob");
}

#[test]
fn sbom_supplier_in_metadata() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--sbom-supplier", "ACME Corp"]);
    assert_eq!(parsed["metadata"]["supplier"]["name"], "ACME Corp");
}

#[test]
fn sbom_no_optional_does_not_break_output() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--no-optional"]);
    assert_eq!(parsed["bomFormat"], "CycloneDX");
    let components = parsed["components"].as_array().expect("components");
    assert!(components.iter().any(|comp| comp["name"] == "is-positive"), "prod dep still present");
}

#[test]
fn sbom_schema_url_matches_spec_version() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--sbom-spec-version", "1.5"]);
    let schema = parsed["$schema"].as_str().expect("$schema");
    assert!(schema.contains("1.5"), "schema should match spec version 1.5, got {schema}");
}

#[test]
fn sbom_spdx_root_has_purpose() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);
    let root = &parsed["packages"].as_array().expect("packages")[0];
    assert_eq!(root["primaryPackagePurpose"], "LIBRARY");
}

#[test]
fn sbom_spdx_application_type_purpose() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "spdx", &["--sbom-type", "application"]);
    let root = &parsed["packages"].as_array().expect("packages")[0];
    assert_eq!(root["primaryPackagePurpose"], "APPLICATION");
}

#[test]
fn sbom_spdx_document_namespace_has_uuid() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);
    let ns = parsed["documentNamespace"].as_str().expect("documentNamespace");
    assert!(ns.contains("spdx.org/spdxdocs/"), "namespace should contain spdx.org");
    let parts: Vec<&str> = ns.rsplitn(2, '-').collect();
    assert!(parts[0].len() >= 8, "namespace should end with UUID-like suffix");
}

#[test]
fn sbom_cyclonedx_scoped_root_has_group() {
    let tmp = copy_fixture("workspace-sbom");
    // workspace-sbom root has name "workspace-sbom-root" (unscoped)
    // but app-a is "@test/app-a" - we need a scoped root to test group
    // Create a temp fixture with scoped name
    fs::write(
        tmp.path().join("package.json"),
        r#"{"name":"@myorg/myapp","version":"2.0.0","license":"MIT"}"#,
    )
    .unwrap();
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    assert_eq!(parsed["metadata"]["component"]["group"], "@myorg");
    assert_eq!(parsed["metadata"]["component"]["name"], "myapp");
}

#[test]
fn sbom_workspace_link_deps_as_components() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let components = parsed["components"].as_array().expect("components");
    let names: Vec<&str> = components.iter().filter_map(|comp| comp["name"].as_str()).collect();
    assert!(names.contains(&"is-positive"), "registry dep should be included");
    assert!(names.contains(&"is-negative"), "registry dep from app-b should be included");
    assert!(names.contains(&"shared-lib"), "workspace link dep should be included as component");
    assert!(names.contains(&"is-odd"), "transitive dep of workspace link should be included");
}

#[test]
fn sbom_workspace_split_produces_multiple_lines() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let output =
        pacquet(tmp.path(), ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--split"])
            .output()
            .expect("run pacquet");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|line| !line.is_empty()).collect();
    assert!(
        lines.len() >= 3,
        "workspace with 4 importers should produce at least 3 NDJSON lines (root may be empty), got {}",
        lines.len(),
    );
    for line in &lines {
        let parsed: serde_json::Value = serde_json::from_str(line).expect("valid JSON");
        assert_eq!(parsed["bomFormat"], "CycloneDX");
    }
}

#[test]
fn sbom_workspace_split_out_writes_files() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let out_pattern = tmp.path().join("out/%s.cdx.json");
    let output = pacquet(
        tmp.path(),
        [
            "sbom",
            "--sbom-format",
            "cyclonedx",
            "--lockfile-only",
            "--split",
            "--out",
            out_pattern.to_str().unwrap(),
        ],
    )
    .output()
    .expect("run pacquet");
    assert!(output.status.success());
    let out_dir = tmp.path().join("out");
    let files: Vec<String> = fs::read_dir(&out_dir)
        .expect("read output dir")
        .filter_map(|entry| entry.ok().map(|entry| entry.file_name().to_string_lossy().to_string()))
        .collect();
    assert!(files.len() >= 3, "should write files for workspace packages, got {files:?}");
}

#[test]
fn sbom_workspace_split_out_percent_v() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let out_pattern = tmp.path().join("out/%s-%v.cdx.json");
    let output = pacquet(
        tmp.path(),
        [
            "sbom",
            "--sbom-format",
            "cyclonedx",
            "--lockfile-only",
            "--split",
            "--out",
            out_pattern.to_str().unwrap(),
        ],
    )
    .output()
    .expect("run pacquet");
    assert!(output.status.success());
    let out_dir = tmp.path().join("out");
    let files: Vec<String> = fs::read_dir(&out_dir)
        .expect("read output dir")
        .filter_map(|entry| entry.ok().map(|entry| entry.file_name().to_string_lossy().to_string()))
        .collect();
    assert!(
        files.iter().any(|file| file.contains("1.0.0")),
        "filenames should contain version: {files:?}",
    );
}

#[test]
fn sbom_workspace_filter_selects_importer() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let output = pacquet(
        tmp.path(),
        ["-F", "app-a", "sbom", "--sbom-format", "cyclonedx", "--lockfile-only"],
    )
    .output()
    .expect("run pacquet");
    assert!(
        output.status.success(),
        "pacquet sbom with filter failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse JSON output");
    let components = parsed["components"].as_array().expect("components");
    let names: Vec<&str> = components.iter().filter_map(|comp| comp["name"].as_str()).collect();
    assert!(names.contains(&"is-positive"), "app-a dep should be included");
    assert!(!names.contains(&"is-negative"), "app-b dep should be excluded by filter");
}

#[test]
fn sbom_workspace_link_dep_has_metadata() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let components = parsed["components"].as_array().expect("components");
    let shared_lib = components.iter().find(|comp| comp["name"] == "shared-lib");
    assert!(shared_lib.is_some(), "shared-lib should be a component");
    let shared_lib = shared_lib.unwrap();
    assert_eq!(shared_lib["version"], "0.1.0");
    assert_eq!(shared_lib["purl"], "pkg:npm/shared-lib@0.1.0");
}

#[test]
fn sbom_workspace_spdx_link_deps() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);
    let packages = parsed["packages"].as_array().expect("packages");
    assert!(
        packages.iter().any(|pkg| pkg["name"] == "shared-lib"),
        "shared-lib should be in SPDX packages",
    );
}

#[test]
fn sbom_missing_lockfile_fails() {
    let tmp = TempDir::new().expect("create temp dir");
    fs::write(tmp.path().join("package.json"), r#"{"name":"no-lockfile","version":"1.0.0"}"#)
        .unwrap();
    let output = pacquet(tmp.path(), ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only"])
        .output()
        .expect("run pacquet");
    assert!(!output.status.success(), "should fail without lockfile");
}

#[test]
fn sbom_prod_scope_undefined_for_prod_components() {
    let tmp = copy_fixture("with-dev-dependency");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let components = parsed["components"].as_array().expect("components");
    let is_positive =
        components.iter().find(|comp| comp["name"] == "is-positive").expect("is-positive");
    assert!(is_positive.get("scope").is_none(), "prod components should not have scope field");
}

#[test]
fn sbom_split_single_project_not_triggered() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    assert!(
        parsed["bomFormat"].is_string(),
        "single project should produce regular JSON, not NDJSON",
    );
}

#[test]
fn sbom_workspace_split_each_line_has_correct_root() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let output =
        pacquet(tmp.path(), ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--split"])
            .output()
            .expect("run pacquet");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let boms: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).expect("valid JSON"))
        .collect();
    let root_names: Vec<&str> =
        boms.iter().filter_map(|bom| bom["metadata"]["component"]["name"].as_str()).collect();
    assert!(root_names.contains(&"app-a"), "split should include app-a");
    assert!(root_names.contains(&"app-b"), "split should include app-b");
}

#[test]
fn sbom_dev_flag_includes_only_dev() {
    let tmp = copy_fixture("with-dev-dependency");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--dev"]);
    let components = parsed["components"].as_array().expect("components");
    assert!(
        components.iter().any(|comp| comp["name"] == "typescript"),
        "dev dep should be included",
    );
    assert!(
        !components.iter().any(|comp| comp["name"] == "is-positive"),
        "prod dep should be excluded with --dev",
    );
}
