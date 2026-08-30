use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::command_env::CommandTestExt;
use std::{collections::HashSet, ffi::OsStr, fs, path::Path, process::Command};
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
        .without_ambient_pnpm_config()
        .with_env("PNPM_CONFIG_REGISTRY", "https://registry.npmjs.org/")
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
fn split_and_filtered_sbom_read_per_project_workspace_lockfiles() {
    let tmp = copy_fixture("simple-sbom");
    let lockfile = fs::read(tmp.path().join("pnpm-lock.yaml")).expect("read fixture lockfile");
    for name in ["project-a", "project-b"] {
        let project_dir = tmp.path().join("packages").join(name);
        fs::create_dir_all(&project_dir).expect("create project dir");
        fs::write(
            project_dir.join("package.json"),
            serde_json::json!({
                "name": name,
                "version": "1.0.0",
                "dependencies": { "is-positive": "3.1.0" },
            })
            .to_string(),
        )
        .expect("write project manifest");
        fs::write(project_dir.join("pnpm-lock.yaml"), &lockfile).expect("write project lockfile");
    }
    fs::remove_file(tmp.path().join("package.json")).expect("remove root manifest");
    fs::remove_file(tmp.path().join("pnpm-lock.yaml")).expect("remove shared lockfile");
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write workspace manifest");

    let split =
        pacquet(tmp.path(), ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--split"])
            .output()
            .expect("run split pacquet sbom");
    assert!(
        split.status.success(),
        "split SBOM failed: {}",
        String::from_utf8_lossy(&split.stderr),
    );
    let mut names = String::from_utf8(split.stdout)
        .expect("split stdout is UTF-8")
        .lines()
        .map(|line| {
            let sbom: serde_json::Value = serde_json::from_str(line).expect("parse NDJSON line");
            assert!(
                sbom["components"]
                    .as_array()
                    .expect("components array")
                    .iter()
                    .any(|component| component["name"] == "is-positive"),
                "split SBOM should include dependencies from its project lockfile",
            );
            sbom["metadata"]["component"]["name"].as_str().expect("root component name").to_string()
        })
        .collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(names, ["project-a", "project-b"]);

    let filtered = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--filter", "project-a"],
    )
    .output()
    .expect("run filtered pacquet sbom");
    assert!(
        filtered.status.success(),
        "filtered SBOM failed: {}",
        String::from_utf8_lossy(&filtered.stderr),
    );
    let sbom: serde_json::Value =
        serde_json::from_slice(&filtered.stdout).expect("parse filtered SBOM");
    assert_eq!(sbom["metadata"]["component"]["name"], "project-a");
    assert!(
        sbom["components"]
            .as_array()
            .expect("components array")
            .iter()
            .any(|component| component["name"] == "is-positive"),
        "filtered SBOM should include dependencies from the selected project's lockfile",
    );
}

#[test]
fn sbom_rejects_conflicting_entries_from_dedicated_lockfiles() {
    let tmp = copy_fixture("simple-sbom");
    let lockfile =
        fs::read_to_string(tmp.path().join("pnpm-lock.yaml")).expect("read fixture lockfile");
    for name in ["project-a", "project-b"] {
        let project_dir = tmp.path().join("packages").join(name);
        fs::create_dir_all(&project_dir).expect("create project dir");
        fs::write(
            project_dir.join("package.json"),
            serde_json::json!({
                "name": name,
                "version": "1.0.0",
                "dependencies": { "is-positive": "3.1.0" },
            })
            .to_string(),
        )
        .expect("write project manifest");
        let project_lockfile = if name == "project-b" {
            lockfile.replace(&["sha512-8N", "D1"].concat(), "sha512-different")
        } else {
            lockfile.clone()
        };
        fs::write(project_dir.join("pnpm-lock.yaml"), project_lockfile)
            .expect("write project lockfile");
    }
    fs::remove_file(tmp.path().join("package.json")).expect("remove root manifest");
    fs::remove_file(tmp.path().join("pnpm-lock.yaml")).expect("remove shared lockfile");
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write workspace manifest");

    let output =
        pacquet(tmp.path(), ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--split"])
            .output()
            .expect("run split pacquet sbom");

    assert!(!output.status.success(), "conflicting lockfile entries must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_SBOM_CONFLICTING_LOCKFILE_ENTRIES"), "stderr: {stderr}");
    let compact_stderr: String = stderr.replace('│', "").split_whitespace().collect();
    assert!(compact_stderr.contains("is-positive@3.1.0"), "stderr: {stderr}");
}

#[test]
fn sbom_merges_snapshot_optionality_from_dedicated_lockfiles() {
    let tmp = copy_fixture("simple-sbom");
    let lockfile =
        fs::read_to_string(tmp.path().join("pnpm-lock.yaml")).expect("read fixture lockfile");
    let lockfile = lockfile.replace(
        "    engines: {node: '>=0.10.0'}",
        "    engines: {node: '>=0.10.0'}\n    os: [unsupported-test-os]",
    );
    for name in ["project-a", "project-b"] {
        let project_dir = tmp.path().join("packages").join(name);
        fs::create_dir_all(&project_dir).expect("create project dir");
        fs::write(
            project_dir.join("package.json"),
            serde_json::json!({
                "name": name,
                "version": "1.0.0",
                "dependencies": { "is-positive": "3.1.0" },
            })
            .to_string(),
        )
        .expect("write project manifest");
        let project_lockfile = if name == "project-b" {
            let project_lockfile = lockfile.replace(
                "  is-positive@3.1.0:\n    dev: false",
                "  is-positive@3.1.0:\n    optional: true\n    dev: false",
            );
            assert_ne!(project_lockfile, lockfile, "mark project-b's snapshot as optional");
            project_lockfile
        } else {
            lockfile.clone()
        };
        fs::write(project_dir.join("pnpm-lock.yaml"), project_lockfile)
            .expect("write project lockfile");
    }
    fs::remove_file(tmp.path().join("package.json")).expect("remove root manifest");
    fs::remove_file(tmp.path().join("pnpm-lock.yaml")).expect("remove shared lockfile");
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write workspace manifest");

    let output = pacquet(tmp.path(), ["sbom", "--sbom-format", "cyclonedx", "--split"])
        .output()
        .expect("run split pacquet sbom");

    assert!(
        output.status.success(),
        "derived snapshot optionality should merge: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let sboms: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .expect("SBOM output is UTF-8")
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse split CycloneDX output"))
        .collect();
    assert_eq!(sboms.len(), 2);
    for sbom in sboms {
        let components = sbom["components"].as_array().expect("components array");
        assert!(
            components.iter().any(|component| component["name"] == "is-positive"),
            "a required platform-incompatible snapshot must remain in the SBOM",
        );
    }
}

fn dedicated_workspace_with_reachable_project() -> TempDir {
    let tmp = copy_fixture("simple-sbom");
    let dependency_lockfile =
        fs::read_to_string(tmp.path().join("pnpm-lock.yaml")).expect("read fixture lockfile");
    let project_a = tmp.path().join("packages/project-a");
    let project_b = tmp.path().join("packages/project-b");
    fs::create_dir_all(&project_a).expect("create project-a");
    fs::create_dir_all(&project_b).expect("create project-b");
    fs::write(
        project_a.join("package.json"),
        serde_json::json!({
            "name": "project-a",
            "version": "1.0.0",
            "dependencies": { "project-b": "workspace:*" },
        })
        .to_string(),
    )
    .expect("write project-a manifest");
    fs::write(
        project_b.join("package.json"),
        serde_json::json!({
            "name": "project-b",
            "version": "1.0.0",
            "dependencies": { "is-positive": "3.1.0" },
        })
        .to_string(),
    )
    .expect("write project-b manifest");
    fs::write(
        project_a.join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      project-b:\n        specifier: workspace:*\n        version: link:../project-b\n",
    )
    .expect("write project-a lockfile");
    fs::write(project_b.join("pnpm-lock.yaml"), dependency_lockfile)
        .expect("write project-b lockfile");
    fs::remove_file(tmp.path().join("package.json")).expect("remove root manifest");
    fs::remove_file(tmp.path().join("pnpm-lock.yaml")).expect("remove shared lockfile");
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write workspace manifest");

    tmp
}

#[test]
fn filtered_sbom_reads_reachable_workspace_project_lockfiles() {
    let tmp = dedicated_workspace_with_reachable_project();

    let output = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--filter", "project-a"],
    )
    .output()
    .expect("run filtered pacquet sbom");

    assert!(
        output.status.success(),
        "filtered SBOM failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let sbom: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse filtered SBOM");
    let component_names: HashSet<&str> = sbom["components"]
        .as_array()
        .expect("components array")
        .iter()
        .filter_map(|component| component["name"].as_str())
        .collect();
    assert!(component_names.contains("project-b"));
    assert!(component_names.contains("is-positive"));
}

#[test]
fn filtered_sbom_rejects_a_reachable_project_without_a_dedicated_lockfile() {
    let tmp = dedicated_workspace_with_reachable_project();
    fs::remove_file(tmp.path().join("packages/project-b/pnpm-lock.yaml"))
        .expect("remove reachable project lockfile");

    let output = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--filter", "project-a"],
    )
    .output()
    .expect("run filtered pacquet sbom");

    assert!(!output.status.success(), "an incomplete workspace graph must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_SBOM_MISSING_IMPORTERS"), "stderr: {stderr}");
    assert!(stderr.contains("packages/project-b"), "stderr: {stderr}");
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
fn sbom_workspace_split_from_member_anchors_importers_at_workspace_root() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let member = tmp.path().join("app-a");
    let output =
        pacquet(&member, ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--split"])
            .output()
            .expect("run pacquet from workspace member");

    assert!(
        output.status.success(),
        "member sbom failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let boms = output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<serde_json::Value>(line).expect("valid JSON"))
        .collect::<Vec<_>>();
    let root_names = boms
        .iter()
        .filter_map(|bom| bom["metadata"]["component"]["name"].as_str())
        .collect::<Vec<_>>();

    assert!(root_names.contains(&"app-a"), "split should include app-a: {root_names:?}");
    assert!(root_names.contains(&"app-b"), "split should include app-b: {root_names:?}");
    assert!(root_names.contains(&"shared-lib"), "split should include shared-lib: {root_names:?}");
}

#[test]
fn sbom_workspace_from_member_uses_the_workspace_root_component() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let parsed = run_sbom_json(&tmp.path().join("app-a"), "cyclonedx", &[]);

    assert_eq!(parsed["metadata"]["component"]["name"], "workspace-sbom-root");
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

/// The names of the root components of a `--split` run's NDJSON lines, in
/// output order — one per selected workspace importer.
fn split_root_names(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let parsed: serde_json::Value = serde_json::from_str(line).expect("valid JSON");
            parsed["metadata"]["component"]["name"].as_str().expect("root name").to_string()
        })
        .collect()
}

/// `--filter <pkg>...` walks every dependency edge, so a workspace
/// project reachable only through `devDependencies` is covered too.
#[test]
fn sbom_filter_selects_dev_dependency_projects() {
    let tmp = copy_fixture("workspace-sbom-filter-prod");
    let output = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--split", "--filter", "app..."],
    )
    .output()
    .expect("run pacquet");
    assert!(
        output.status.success(),
        "pacquet sbom failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(split_root_names(&String::from_utf8_lossy(&output.stdout)), ["app", "dev-lib"]);
}

/// `--filter-prod <pkg>...` walks production dependencies only, so
/// `dev-lib` — a `devDependencies`-only workspace dependency of `app` —
/// is left out.
#[test]
fn sbom_filter_prod_follows_production_deps_only() {
    let tmp = copy_fixture("workspace-sbom-filter-prod");
    let output = pacquet(
        tmp.path(),
        [
            "sbom",
            "--sbom-format",
            "cyclonedx",
            "--lockfile-only",
            "--split",
            "--filter-prod",
            "app...",
        ],
    )
    .output()
    .expect("run pacquet");
    assert!(
        output.status.success(),
        "pacquet sbom failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(split_root_names(&String::from_utf8_lossy(&output.stdout)), ["app"]);
}

/// Selectors that match no workspace project skip the command entirely:
/// pnpm prints the notice and writes no SBOM, exiting zero.
#[test]
fn sbom_filter_matching_nothing_writes_no_sbom() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let output = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--filter", "no-such-package"],
    )
    .output()
    .expect("run pacquet");
    assert!(output.status.success(), "no match alone must not fail the run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("No projects matched the filters in"), "stdout:\n{stdout}");
    assert!(!stdout.contains("bomFormat"), "no SBOM should be written:\n{stdout}");
}

/// `--fail-if-no-match` turns the same empty selection into an exit-code-1
/// failure.
#[test]
fn sbom_fail_if_no_match_exits_non_zero() {
    let tmp = copy_fixture("workspace-sbom-populated");
    let output = pacquet(
        tmp.path(),
        [
            "sbom",
            "--sbom-format",
            "cyclonedx",
            "--lockfile-only",
            "--filter-prod",
            "no-such-package",
            "--fail-if-no-match",
        ],
    )
    .output()
    .expect("run pacquet");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("No projects matched the filters in"), "stdout:\n{stdout}");
}

/// `--workspace-root` narrows the SBOM to the root project, even though
/// the workspace package patterns don't name it — the root project is
/// always part of the workspace, so the `{<workspace-root>}` selector the
/// flag adds finds it.
#[test]
fn sbom_workspace_root_selects_only_the_root() {
    let tmp = copy_fixture("workspace-sbom-filter-prod");
    let output = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--workspace-root"],
    )
    .output()
    .expect("run pacquet");
    assert!(
        output.status.success(),
        "pacquet sbom failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse JSON output");
    assert_eq!(parsed["metadata"]["component"]["name"], "workspace-sbom-filter-prod-root");
    assert!(
        parsed["components"].as_array().expect("components").is_empty(),
        "the root project has no dependencies, so nothing from app / dev-lib may leak in: {}",
        parsed["components"],
    );
}

/// A lockfile with no importer for a selected project is out of date, and
/// walking what is left would under-report the selection's dependencies —
/// so the run fails instead of writing that SBOM.
#[test]
fn sbom_fails_when_the_lockfile_has_no_importer_for_a_selected_project() {
    let tmp = copy_fixture("workspace-sbom-filter-prod");
    let added = tmp.path().join("newpkg");
    fs::create_dir_all(&added).expect("create the added package dir");
    fs::write(added.join("package.json"), r#"{ "name": "newpkg", "version": "1.0.0" }"#)
        .expect("write the added package.json");
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - app\n  - dev-lib\n  - newpkg\n",
    )
    .expect("write pnpm-workspace.yaml");

    let output = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--filter", "newpkg"],
    )
    .output()
    .expect("run pacquet");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "stdout:\n{stdout}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_SBOM_MISSING_IMPORTERS"), "stderr:\n{stderr}");
    assert!(stderr.contains("newpkg"), "the error should name the missing project:\n{stderr}");
    assert!(!stdout.contains("bomFormat"), "no SBOM may be written for an out-of-date lockfile");
}

/// No lockfile at all is a different failure from a lockfile that is merely
/// out of date, and keeps its own error even under a `--filter`.
#[test]
fn sbom_without_a_lockfile_reports_the_missing_lockfile_not_missing_importers() {
    let tmp = copy_fixture("workspace-sbom-filter-prod");
    fs::remove_file(tmp.path().join("pnpm-lock.yaml")).expect("remove the lockfile");

    let output = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--filter", "app"],
    )
    .output()
    .expect("run pacquet");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_SBOM_NO_LOCKFILE"), "stderr:\n{stderr}");
}
