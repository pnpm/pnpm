use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    command_env::CommandTestExt,
};
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
    parse_sbom_output(&pacquet(workspace, args).output().expect("run pacquet"))
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
fn sbom_missing_format_fails() {
    let tmp = copy_fixture("simple-sbom");
    let output = pacquet(tmp.path(), ["sbom"]).output().expect("run pacquet");
    assert!(!output.status.success());
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
fn sbom_application_type() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--sbom-type", "application"]);
    assert_eq!(parsed["metadata"]["component"]["type"], "application");
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

/// Two packages whose names render to the same `--out` path collide. The
/// run fails, and the SBOM written first stays on disk.
#[test]
fn sbom_split_out_collision_keeps_the_first_file() {
    let tmp = copy_fixture("simple-sbom");
    let lockfile = fs::read(tmp.path().join("pnpm-lock.yaml")).expect("read fixture lockfile");
    // `@a/b` and `a-b` both sanitize to `a-b`.
    for (dir, name) in [("scoped", "@a/b"), ("plain", "a-b")] {
        let project_dir = tmp.path().join("packages").join(dir);
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

    let listed =
        pacquet(tmp.path(), ["sbom", "--sbom-format", "cyclonedx", "--lockfile-only", "--split"])
            .output()
            .expect("run pacquet sbom --split");
    assert!(listed.status.success(), "{}", String::from_utf8_lossy(&listed.stderr));
    let first_name = split_root_names(&String::from_utf8_lossy(&listed.stdout))
        .first()
        .expect("at least one project")
        .clone();

    let output = pacquet(
        tmp.path(),
        [
            "sbom",
            "--sbom-format",
            "cyclonedx",
            "--lockfile-only",
            "--split",
            "--out",
            "out/%s.json",
        ],
    )
    .output()
    .expect("run pacquet sbom --split --out");
    assert!(!output.status.success(), "colliding output paths must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_SBOM_OUT_PATH_COLLISION"), "{stderr}");
    let written: serde_json::Value = serde_json::from_slice(
        &fs::read(tmp.path().join("out/a-b.json")).expect("read the SBOM written first"),
    )
    .expect("parse the written SBOM");
    assert_eq!(written["metadata"]["component"]["name"], first_name);
}

#[test]
fn sbom_lifecycle_pre_build_in_lockfile_only() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let phase = parsed["metadata"]["lifecycles"][0]["phase"].as_str().expect("phase");
    assert_eq!(phase, "pre-build");
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

fn run_sbom_json_from_store(workspace: &Path, format: &str) -> serde_json::Value {
    parse_sbom_output(
        &pacquet(workspace, ["sbom", "--sbom-format", format]).output().expect("run pacquet"),
    )
}

fn parse_sbom_output(output: &std::process::Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "pacquet sbom failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    serde_json::from_slice(&output.stdout).expect("parse JSON output")
}

/// The npm `owner/repo` shorthand in the root manifest's `repository` field
/// is not an iri-reference, so `CycloneDX` consumers such as Dependency-Track
/// reject the SBOM. It is expanded to the GitHub URL npm derives for it.
#[test]
fn sbom_root_repository_shorthand_is_expanded_to_github_url() {
    let tmp = copy_fixture("sbom-repository");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let root = &parsed["metadata"]["component"];
    let ext_refs = root["externalReferences"].as_array().expect("root externalReferences");
    let vcs = ext_refs.iter().find(|ext_ref| ext_ref["type"] == "vcs").expect("vcs reference");
    assert_eq!(vcs["url"], "git+https://github.com/acme/sbom-repository-test.git");
}

/// The same shorthand on the SPDX side lands in the root package's
/// `homepage`, which SPDX also requires to be a valid URL.
#[test]
fn sbom_spdx_root_repository_shorthand_is_expanded_to_github_url() {
    let tmp = copy_fixture("sbom-repository");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);
    let root = &parsed["packages"].as_array().expect("packages")[0];
    assert_eq!(root["homepage"], "git+https://github.com/acme/sbom-repository-test.git");
}

/// A `repository` value that is neither an absolute URL nor the two-segment
/// shorthand (an scp-style git remote here) is dropped instead of published
/// as a broken URL.
#[test]
fn sbom_root_repository_that_is_not_a_url_is_omitted() {
    let tmp = copy_fixture("sbom-repository");
    fs::write(
        tmp.path().join("package.json"),
        r#"{
  "name": "sbom-repository-test",
  "version": "1.0.0",
  "license": "ISC",
  "repository": "git@github.com:foo/bar.git",
  "dependencies": { "is-positive": "^3.1.0" }
}"#,
    )
    .expect("write package.json");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let root = &parsed["metadata"]["component"];
    assert!(
        !root
            .get("externalReferences")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|ext_refs| ext_refs.iter().any(|ext_ref| ext_ref["type"] == "vcs")),
        "an scp-style remote is not an iri-reference and must not be published: {root}",
    );
}

/// A component's `repository` shorthand is expanded the same way: the
/// fixture package's manifest carries `"repository": "pnpm/sbom-shorthand-repo"`.
#[test]
fn sbom_component_repository_shorthand_is_expanded_to_github_url() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let registry = mock_instance.url();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "sbom-component-repository",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/sbom-shorthand-repo": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_args(["install"]).with_arg(format!("--registry={registry}")).assert().success();

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .without_ambient_pnpm_config()
        .with_args(["sbom", "--sbom-format", "cyclonedx"])
        .with_arg(format!("--registry={registry}"))
        .output()
        .expect("run pacquet sbom");
    assert!(
        output.status.success(),
        "pacquet sbom failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).expect("parse SBOM");
    let component = parsed["components"]
        .as_array()
        .expect("components array")
        .iter()
        .find(|component| {
            component["name"] == "sbom-shorthand-repo" && component["version"] == "1.0.0"
        })
        .expect("@pnpm.e2e/sbom-shorthand-repo component");
    let ext_refs =
        component["externalReferences"].as_array().expect("component externalReferences");
    let vcs = ext_refs.iter().find(|ext_ref| ext_ref["type"] == "vcs").expect("vcs reference");
    assert_eq!(vcs["url"], "git+https://github.com/pnpm/sbom-shorthand-repo.git");

    drop((root, mock_instance));
}

/// Gives the root manifest the string form of the `author` field.
fn set_root_author(workspace: &Path, author: &str) {
    let manifest_path = workspace.join("package.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read the root manifest"))
            .expect("parse the root manifest");
    manifest["author"] = serde_json::Value::String(author.to_string());
    fs::write(manifest_path, manifest.to_string()).expect("write the root manifest");
}

/// Plants the store copy of the fixture's only dependency, with the object
/// form of the `author` field. A run that is not `--lockfile-only` reads
/// dependency metadata from there, so this is the seam for giving a dependency
/// an author.
fn set_dependency_author(workspace: &Path, author_name: &str) {
    let package_dir =
        workspace.join("node_modules/.pnpm/is-positive@3.1.0/node_modules/is-positive");
    fs::create_dir_all(&package_dir).expect("create the package directory");
    fs::write(
        package_dir.join("package.json"),
        serde_json::json!({
            "name": "is-positive",
            "version": "3.1.0",
            "description": "sbom author fixture",
            "author": { "name": author_name },
        })
        .to_string(),
    )
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

fn spdx_package<'a>(document: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    document["packages"]
        .as_array()
        .expect("packages array")
        .iter()
        .find(|package| package["name"] == name)
        .unwrap_or_else(|| panic!("find the {name} package"))
}

mod workspace;

mod metadata;
