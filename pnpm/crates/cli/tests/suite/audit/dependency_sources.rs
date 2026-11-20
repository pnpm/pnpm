use super::{
    CommandTempCwd, Matcher, advisory_entry, advisory_response, assert_failure, assert_success, fs,
    pacquet_cmd,
    signatures::{
        keys_mock, packument_body, public_key_b64, sign_b64, signatures_json, signing_key,
    },
    stderr, stdout, write_minimal_manifest,
};
use crate::_utils::{append_workspace_yaml_key, gzipped_tarball};
use assert_cmd::assert::OutputAssertExt;
use pnpm_testing_utils::{bin::AddMockedRegistry, git_repo::GitRepoFixture};

#[test]
fn audit_reports_vulnerabilities_after_installing_tarball_file_git_and_named_registry_deps() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpr_url = npmrc_info.mock_instance.url();

    let mut http_server = mockito::Server::new();
    let http_tgz =
        gzipped_tarball(&[("package/package.json", r#"{"name":"vuln-http","version":"1.0.0"}"#)]);
    let tarball_mock = http_server
        .mock("GET", "/tarballs/vuln-http-1.0.0.tgz")
        .with_status(200)
        .with_header("content-type", "application/octet-stream")
        .with_body(http_tgz)
        .create();

    let file_tgz_dir = workspace.join("tarballs");
    fs::create_dir_all(&file_tgz_dir).expect("create tarballs directory");
    let file_tgz =
        gzipped_tarball(&[("package/package.json", r#"{"name":"vuln-file","version":"1.0.0"}"#)]);
    fs::write(file_tgz_dir.join("vuln-file-1.0.0.tgz"), file_tgz).expect("write local tarball");

    let git_repo = GitRepoFixture::init(root.path(), "vuln-git");
    git_repo.write_file("package.json", r#"{"name":"vuln-git","version":"1.0.0"}"#);
    let commit = git_repo.commit("initial");
    let git_spec = git_repo.git_url_at(&commit);

    append_workspace_yaml_key(&workspace, "namedRegistries", format!("{{ work: '{pnpr_url}' }}"));
    append_workspace_yaml_key(&workspace, "registry", format!("{}/", http_server.url()));

    let manifest = serde_json::json!({
        "name": "audit-sources-app",
        "version": "1.0.0",
        "dependencies": {
            "vuln-http": format!("{}/tarballs/vuln-http-1.0.0.tgz", http_server.url()),
            "vuln-file": "file:./tarballs/vuln-file-1.0.0.tgz",
            "vuln-git": git_spec,
            "@pnpm.e2e/foo": "work:1.0.0",
        },
    });
    fs::write(
        workspace.join("package.json"),
        serde_json::to_string_pretty(&manifest).expect("serialize package.json"),
    )
    .expect("write package.json");

    pacquet_cmd(&workspace, ["install"]).assert().success();
    tarball_mock.assert();

    let bulk_response = format!(
        "{{\n{},\n{},\n{},\n{}\n}}",
        advisory_entry(
            "vuln-http",
            1001,
            "critical",
            "<=1.0.0",
            "HTTP tarball vuln",
            "GHSA-1111-2222-3331"
        ),
        advisory_entry(
            "vuln-file",
            1002,
            "high",
            "<=1.0.0",
            "File tarball vuln",
            "GHSA-1111-2222-3332"
        ),
        advisory_entry("vuln-git", 1003, "high", "<=1.0.0", "Git dep vuln", "GHSA-1111-2222-3333"),
        advisory_entry(
            "@pnpm.e2e/foo",
            1004,
            "high",
            "<=1.0.0",
            "Named registry vuln",
            "GHSA-1111-2222-3334"
        ),
    );
    let expected_payload = serde_json::json!({
        "@pnpm.e2e/foo": ["1.0.0"],
        "vuln-file": ["1.0.0"],
        "vuln-git": ["1.0.0"],
        "vuln-http": ["1.0.0"],
    });
    let audit_mock = http_server
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .match_body(Matcher::Json(expected_payload))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(bulk_response)
        .create();

    let output =
        pacquet_cmd(&workspace, ["audit", "--json"]).output().expect("run pnpm audit --json");
    let out = stdout(&output);
    assert_eq!(output.status.code(), Some(1), "stdout:\n{out}");

    let report: serde_json::Value = serde_json::from_str(&out).expect("parse audit JSON");
    assert_eq!(report["metadata"]["totalDependencies"], 4);
    assert_eq!(report["metadata"]["dependencies"], 4);
    assert_eq!(report["metadata"]["vulnerabilities"]["critical"], 1);
    assert_eq!(report["metadata"]["vulnerabilities"]["high"], 3);
    assert_eq!(report["advisories"]["1001"]["findings"][0]["paths"][0], ".>vuln-http");
    assert_eq!(report["advisories"]["1002"]["findings"][0]["paths"][0], ".>vuln-file");
    assert_eq!(report["advisories"]["1003"]["findings"][0]["paths"][0], ".>vuln-git");
    assert_eq!(report["advisories"]["1004"]["findings"][0]["paths"][0], ".>@pnpm.e2e/foo");
    audit_mock.assert();
    drop((root, npmrc_info));
}

#[test]
fn audit_fix_update_re_resolves_vulnerable_named_registry_dependency() {
    let CommandTempCwd { workspace, root, npmrc_info, .. }: CommandTempCwd<AddMockedRegistry> =
        CommandTempCwd::init().add_mocked_registry();
    let pnpr_url = npmrc_info.mock_instance.url();
    let mut audit_registry = mockito::Server::new();

    append_workspace_yaml_key(&workspace, "namedRegistries", format!("{{ work: '{pnpr_url}' }}"));
    append_workspace_yaml_key(&workspace, "registry", format!("{}/", audit_registry.url()));

    let write_pkg = |spec: &str| {
        let manifest = serde_json::json!({
            "name": "named-fix-update",
            "version": "1.0.0",
            "dependencies": {
                "@pnpm.e2e/multi-version-a": spec,
            },
        });
        fs::write(
            workspace.join("package.json"),
            serde_json::to_string_pretty(&manifest).expect("serialize package.json"),
        )
        .expect("write package.json");
    };
    write_pkg("work:2.0.0");
    pacquet_cmd(&workspace, ["install"]).assert().success();
    write_pkg("work:^2.0.0");
    pacquet_cmd(&workspace, ["install"]).assert().success();

    let lockfile_before =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile before fix");
    assert!(lockfile_before.contains("@pnpm.e2e/multi-version-a@work:2.0.0"));

    let default_packument_mock = audit_registry
        .mock("GET", "/@pnpm.e2e%2fmulti-version-a")
        .expect(0)
        .create();
    let mock = audit_registry
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(advisory_response(
            "@pnpm.e2e/multi-version-a",
            9101,
            "high",
            ">=2.0.0 <2.1.0",
            "vulnerable 2.0.x",
            "GHSA-name-1111-2222",
        ))
        .create();

    let output = pacquet_cmd(&workspace, ["audit", "--fix", "update"])
        .output()
        .expect("run audit --fix update");
    let out = stdout(&output);
    assert_success(&output);
    assert!(out.contains("1 vulnerability was fixed, 0 vulnerabilities remain."), "{out}");

    let lockfile_after =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile after fix");
    assert!(lockfile_after.contains("@pnpm.e2e/multi-version-a@work:2.1.0"));
    assert!(!lockfile_after.contains("@pnpm.e2e/multi-version-a@work:2.0.0"));
    default_packument_mock.assert();
    mock.assert();
    drop((root, npmrc_info));
}

#[test]
fn audit_signatures_verifies_named_registry_packages_and_skips_non_registry_deps() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init();
    let mut default_registry = mockito::Server::new();
    let mut work_registry = mockito::Server::new();

    let default_key = signing_key();
    let work_key =
        p256::ecdsa::SigningKey::from_slice(&[0x24; 32]).expect("valid second P-256 scalar");
    let integrity_default = "sha512-YWJj";
    let integrity_work = "sha512-ZGVm";

    let default_keys_mock =
        keys_mock(&mut default_registry, &public_key_b64(&default_key)).create();
    let work_keys_mock = keys_mock(&mut work_registry, &public_key_b64(&work_key)).create();

    let default_sig = sign_b64(&default_key, &format!("signed-default@1.0.0:{integrity_default}"));
    let default_packument = default_registry
        .mock("GET", "/signed-default")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument_body(
            "signed-default",
            "1.0.0",
            integrity_default,
            &signatures_json(&default_sig),
        ))
        .expect(1)
        .create();

    let work_sig = sign_b64(&work_key, &format!("signed-work@1.0.0:{integrity_work}"));
    let work_packument = work_registry
        .mock("GET", "/signed-work")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument_body(
            "signed-work",
            "1.0.0",
            integrity_work,
            &signatures_json(&work_sig),
        ))
        .expect(1)
        .create();

    let skipped_http = default_registry
        .mock("GET", "/http-pkg")
        .expect(0)
        .create();
    let skipped_file = default_registry
        .mock("GET", "/file-pkg")
        .expect(0)
        .create();
    let skipped_git = default_registry
        .mock("GET", "/git-pkg")
        .expect(0)
        .create();
    let skipped_ws = default_registry
        .mock("GET", "/ws-pkg")
        .expect(0)
        .create();

    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", default_registry.url()))
        .expect("write .npmrc");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("fetchRetries: 0\nnamedRegistries:\n  work: {}/\n", work_registry.url()),
    )
    .expect("write pnpm-workspace.yaml");
    write_minimal_manifest(&workspace);
    fs::write(
        workspace.join("pnpm-lock.yaml"),
        format!(
            "
lockfileVersion: '9.0'

importers:
  .:
    dependencies:
      signed-default:
        specifier: 1.0.0
        version: 1.0.0
      signed-work:
        specifier: work:1.0.0
        version: work:1.0.0
      http-pkg:
        specifier: https://example.com/http-pkg-1.0.0.tgz
        version: https://example.com/http-pkg-1.0.0.tgz
      file-pkg:
        specifier: file:./file-pkg-1.0.0.tgz
        version: file:./file-pkg-1.0.0.tgz
      git-pkg:
        specifier: git+https://example.com/git-pkg.git#deadbeef
        version: git+https://example.com/git-pkg.git#deadbeef
      ws-pkg:
        specifier: workspace:*
        version: file:packages/ws-pkg

packages:
  signed-default@1.0.0:
    resolution: {{integrity: {integrity_default}}}
  signed-work@work:1.0.0:
    resolution: {{integrity: {integrity_work}}}
  http-pkg@https://example.com/http-pkg-1.0.0.tgz:
    resolution: {{tarball: https://example.com/http-pkg-1.0.0.tgz}}
    version: 1.0.0
  file-pkg@file:./file-pkg-1.0.0.tgz:
    resolution: {{tarball: file:./file-pkg-1.0.0.tgz}}
    version: 1.0.0
  git-pkg@git+https://example.com/git-pkg.git#deadbeef:
    resolution: {{repo: https://example.com/git-pkg.git, commit: deadbeef, type: git}}
    version: 1.0.0
  ws-pkg@file:packages/ws-pkg:
    resolution: {{directory: packages/ws-pkg, type: directory}}

snapshots:
  signed-default@1.0.0: {{}}
  signed-work@work:1.0.0: {{}}
  http-pkg@https://example.com/http-pkg-1.0.0.tgz: {{}}
  file-pkg@file:./file-pkg-1.0.0.tgz: {{}}
  git-pkg@git+https://example.com/git-pkg.git#deadbeef: {{}}
  ws-pkg@file:packages/ws-pkg: {{}}
",
        ),
    )
    .expect("write pnpm-lock.yaml");

    let output = pacquet
        .args(["audit", "signatures", "--json"])
        .output()
        .expect("run audit signatures --json");
    let out = stdout(&output);
    assert_success(&output);
    let result: serde_json::Value = serde_json::from_str(&out).expect("parse signatures JSON");
    assert_eq!(result["audited"], 2);
    assert_eq!(result["verified"], 2);
    assert_eq!(
        result["invalid"]
            .as_array()
            .expect("invalid array")
            .len(),
        0,
    );
    assert_eq!(
        result["missing"]
            .as_array()
            .expect("missing array")
            .len(),
        0,
    );

    default_keys_mock.assert();
    work_keys_mock.assert();
    default_packument.assert();
    work_packument.assert();
    skipped_http.assert();
    skipped_file.assert();
    skipped_git.assert();
    skipped_ws.assert();
}

#[test]
fn audit_signatures_fails_when_named_registry_prefix_is_undeclared() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "fetchRetries: 0\n")
        .expect("write pnpm-workspace.yaml");
    write_minimal_manifest(&workspace);
    fs::write(
        workspace.join("pnpm-lock.yaml"),
        "
lockfileVersion: '9.0'

importers:
  .:
    dependencies:
      signed-work:
        specifier: work:1.0.0
        version: work:1.0.0

packages:
  signed-work@work:1.0.0:
    resolution: {integrity: sha512-YWJj}

snapshots:
  signed-work@work:1.0.0: {}
",
    )
    .expect("write pnpm-lock.yaml");

    let output = pacquet
        .args(["audit", "signatures"])
        .output()
        .expect("run audit signatures");
    let err = stderr(&output);
    assert_failure(&output);
    assert!(err.contains("ERR_PNPM_MISSING_NAMED_REGISTRY"), "{err}");
    assert!(err.contains("signed-work@work:1.0.0"), "{err}");
}

#[test]
fn audit_signatures_redacts_credentials_from_named_registry_urls_in_issues() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init();
    let mut work_registry = mockito::Server::new();
    let work_key = signing_key();
    let work_keys_mock = keys_mock(&mut work_registry, &public_key_b64(&work_key)).create();
    let work_packument = work_registry
        .mock("GET", "/unsigned-work")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument_body("unsigned-work", "1.0.0", "sha512-YWJj", "[]"))
        .create();

    let host_port = work_registry.host_with_port();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "fetchRetries: 0\nnamedRegistries:\n  secret: http://build-user:build-token@{host_port}/\n",
        ),
    )
    .expect("write pnpm-workspace.yaml");
    write_minimal_manifest(&workspace);
    fs::write(
        workspace.join("pnpm-lock.yaml"),
        "
lockfileVersion: '9.0'

importers:
  .:
    dependencies:
      unsigned-work:
        specifier: secret:1.0.0
        version: secret:1.0.0

packages:
  unsigned-work@secret:1.0.0:
    resolution: {integrity: sha512-YWJj}

snapshots:
  unsigned-work@secret:1.0.0: {}
",
    )
    .expect("write pnpm-lock.yaml");

    let output = pacquet
        .args(["audit", "signatures", "--json"])
        .output()
        .expect("run audit signatures --json");
    let out = stdout(&output);
    let err = stderr(&output);
    assert_eq!(output.status.code(), Some(1), "stdout:\n{out}\nstderr:\n{err}");
    assert!(!out.contains("build-user"));
    assert!(!out.contains("build-token"));
    assert!(!err.contains("build-user"));
    assert!(!err.contains("build-token"));

    let result: serde_json::Value = serde_json::from_str(&out).expect("parse signatures JSON");
    assert_eq!(result["missing"][0]["registry"], format!("http://{host_port}/"));
    work_keys_mock.assert();
    work_packument.assert();
}
