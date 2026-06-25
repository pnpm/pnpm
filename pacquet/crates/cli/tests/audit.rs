use mockito::Matcher;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{fs, path::Path, process::Output};

#[test]
fn audit_json_posts_bulk_request_and_exits_on_vulnerability() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = registry
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .match_body(mockito::Matcher::PartialJsonString(r#"{"vulnerable":["1.0.0"]}"#.to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
  "vulnerable": [
    {
      "id": 123,
      "url": "https://github.com/advisories/GHSA-test-1111-2222",
      "title": "test vulnerability",
      "severity": "high",
      "vulnerable_versions": "<2.0.0",
      "cwe": "CWE-79"
    }
  ]
}"#,
        )
        .create();

    write_audit_workspace(&workspace, &registry.url(), "");

    let output = pacquet.arg("audit").arg("--json").output().expect("run pacquet audit");

    assert_eq!(output.status.code(), Some(1), "vulnerability should produce exit code 1");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let report: serde_json::Value = serde_json::from_str(&stdout).expect("audit JSON output");
    assert_eq!(report["advisories"]["123"]["title"], "test vulnerability");
    assert_eq!(report["advisories"]["123"]["module_name"], "vulnerable");
    assert_eq!(report["advisories"]["123"]["findings"][0]["paths"][0], ".>vulnerable");
    assert_eq!(report["metadata"]["vulnerabilities"]["high"], 1);
    mock.assert();
}

#[test]
fn audit_no_vulnerabilities_exits_successfully() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(&mut registry, "{}").create();
    write_audit_workspace(&workspace, &registry.url(), "");

    let output = pacquet.arg("audit").output().expect("run pacquet audit");

    assert_success(&output);
    assert_eq!(stdout(&output), "No known vulnerabilities found\n");
    mock.assert();
}

#[test]
fn audit_dev_reports_only_dev_dependencies() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = registry
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .match_body(Matcher::PartialJsonString(r#"{"dev-vulnerable":["1.0.0"]}"#.to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(advisory_response(
            "dev-vulnerable",
            124,
            "high",
            "<2.0.0",
            "dev vulnerability",
            "GHSA-devv-1111-2222",
        ))
        .create();
    write_audit_workspace(&workspace, &registry.url(), "");

    let output =
        pacquet.arg("audit").arg("--dev").arg("--json").output().expect("run pacquet audit");

    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(report["advisories"]["124"]["module_name"], "dev-vulnerable");
    assert_eq!(report["advisories"]["124"]["findings"][0]["dev"], true);
    assert_eq!(report["advisories"]["124"]["findings"][0]["paths"][0], ".>dev-vulnerable");
    mock.assert();
}

#[test]
fn audit_exits_zero_when_every_vulnerability_is_below_audit_level() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        &advisory_response(
            "vulnerable",
            125,
            "moderate",
            "<2.0.0",
            "moderate vulnerability",
            "GHSA-modr-1111-2222",
        ),
    )
    .create();
    write_audit_workspace(&workspace, &registry.url(), "");

    let output =
        pacquet.arg("audit").arg("--audit-level").arg("high").output().expect("run pacquet audit");

    assert_success(&output);
    assert_eq!(stdout(&output), "1 vulnerabilities found\nSeverity: 1 moderate");
    mock.assert();
}

#[test]
fn audit_json_filters_advisories_by_audit_level() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        r#"{
  "vulnerable": [
    {
      "id": 201,
      "url": "https://github.com/advisories/GHSA-loww-1111-2222",
      "title": "low vulnerability",
      "severity": "low",
      "vulnerable_versions": "<2.0.0"
    },
    {
      "id": 202,
      "url": "https://github.com/advisories/GHSA-high-1111-2222",
      "title": "high vulnerability",
      "severity": "high",
      "vulnerable_versions": "<2.0.0"
    },
    {
      "id": 203,
      "url": "https://github.com/advisories/GHSA-crit-1111-2222",
      "title": "critical vulnerability",
      "severity": "critical",
      "vulnerable_versions": "<2.0.0"
    }
  ]
}"#,
    )
    .create();
    write_audit_workspace(&workspace, &registry.url(), "");

    let output = pacquet
        .arg("audit")
        .arg("--json")
        .arg("--audit-level")
        .arg("high")
        .output()
        .expect("run pacquet audit");

    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    let advisories = report["advisories"].as_object().unwrap();
    assert!(!advisories.contains_key("201"));
    assert_eq!(advisories["202"]["severity"], "high");
    assert_eq!(advisories["203"]["severity"], "critical");
    mock.assert();
}

#[test]
fn audit_ignore_registry_errors_keeps_exit_code_zero() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = registry
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body("Something bad happened \u{1b}[31m\n")
        .create();
    write_audit_workspace(&workspace, &registry.url(), "");

    let output =
        pacquet.arg("audit").arg("--ignore-registry-errors").output().expect("run pacquet audit");

    assert_success(&output);
    assert_eq!(stdout(&output), "");
    let stderr = stderr(&output);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(stderr.contains("responded with 500"));
    assert!(stderr.contains(r"Something bad happened \u{1b}[31m\u{a}"));
    assert!(!stderr.contains('\u{1b}'));
    mock.assert();
}

#[test]
fn audit_json_ignore_registry_errors_keeps_stdout_parseable() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = registry
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .with_status(500)
        .with_header("content-type", "application/json")
        .with_body("bad \u{1b}[31m")
        .create();
    write_audit_workspace(&workspace, &registry.url(), "");

    let output = pacquet
        .arg("audit")
        .arg("--json")
        .arg("--ignore-registry-errors")
        .output()
        .expect("run pacquet audit");

    assert_success(&output);
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(report["advisories"].as_object().unwrap().len(), 0);
    assert_eq!(report["metadata"]["vulnerabilities"]["high"], 0);
    assert_eq!(report["metadata"]["dependencies"], 3);
    assert_eq!(report["metadata"]["devDependencies"], 1);
    assert_eq!(report["metadata"]["optionalDependencies"], 1);
    assert_eq!(report["metadata"]["totalDependencies"], 5);
    let stderr = stderr(&output);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(stderr.contains(r"bad \u{1b}[31m"));
    assert!(!stderr.contains('\u{1b}'));
    mock.assert();
}

#[test]
fn audit_sends_auth_token() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = registry
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .match_header("authorization", "Bearer 123")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();
    write_audit_workspace_with_npmrc(
        &workspace,
        &format!("registry={}/\n{}/:_authToken=123\n", registry.url(), nerf(&registry.url())),
        "",
    );

    let output = pacquet.arg("audit").output().expect("run pacquet audit");

    assert_success(&output);
    assert_eq!(stdout(&output), "No known vulnerabilities found\n");
    mock.assert();
}

#[test]
fn audit_omits_authorization_header_without_credentials() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = registry
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .match_header("authorization", Matcher::Missing)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();
    write_audit_workspace(&workspace, &registry.url(), "");

    let output = pacquet.arg("audit").output().expect("run pacquet audit");

    assert_success(&output);
    mock.assert();
}

#[test]
fn audit_endpoint_not_exists_reports_dedicated_error() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = registry
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .with_status(404)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .create();
    write_audit_workspace(&workspace, &registry.url(), "");

    let output = pacquet.arg("audit").output().expect("run pacquet audit");

    assert_failure(&output);
    assert!(stderr(&output).contains("ERR_PNPM_AUDIT_ENDPOINT_NOT_EXISTS"));
    assert!(stderr(&output).contains("doesn't exist"));
    mock.assert();
}

#[test]
fn audit_invalid_json_reports_bad_response() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(&mut registry, "not json <html>").create();
    write_audit_workspace(&workspace, &registry.url(), "");

    let output = pacquet.arg("audit").output().expect("run pacquet audit");

    assert_failure(&output);
    assert!(stderr(&output).contains("ERR_PNPM_AUDIT_BAD_RESPONSE"));
    assert!(stderr(&output).contains("invalid JSON"));
    assert!(stderr(&output).contains("not json <html>"));
    mock.assert();
}

#[test]
fn audit_unexpected_json_body_reports_bad_response() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(&mut registry, "[]").create();
    write_audit_workspace(&workspace, &registry.url(), "");

    let output = pacquet.arg("audit").output().expect("run pacquet audit");

    assert_failure(&output);
    assert!(stderr(&output).contains("ERR_PNPM_AUDIT_BAD_RESPONSE"));
    assert!(stderr(&output).contains("unexpected body"));
    mock.assert();
}

#[test]
fn audit_ignores_configured_ghsas_in_text_report() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        &advisory_response(
            "vulnerable",
            301,
            "high",
            "<2.0.0",
            "ignored vulnerability",
            "GHSA-ignr-1111-2222",
        ),
    )
    .create();
    write_audit_workspace(
        &workspace,
        &registry.url(),
        "auditConfig:\n  ignoreGhsas:\n    - GHSA-ignr-1111-2222\n",
    );

    let output =
        pacquet.arg("audit").arg("--audit-level").arg("moderate").output().expect("run pacquet");

    assert_success(&output);
    let stdout = stdout(&output);
    assert!(!stdout.contains("ignored vulnerability"));
    assert!(stdout.contains("1 high (1 ignored)"));
    mock.assert();
}

#[test]
fn audit_ignores_configured_ghsas_in_json_report() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        r#"{
  "vulnerable": [
    {
      "id": 401,
      "url": "https://github.com/advisories/GHSA-ignr-1111-2222",
      "title": "ignored vulnerability",
      "severity": "high",
      "vulnerable_versions": "<2.0.0"
    },
    {
      "id": 402,
      "url": "https://github.com/advisories/GHSA-visi-1111-2222",
      "title": "visible vulnerability",
      "severity": "critical",
      "vulnerable_versions": "<2.0.0"
    }
  ]
}"#,
    )
    .create();
    write_audit_workspace(
        &workspace,
        &registry.url(),
        "auditConfig:\n  ignoreGhsas:\n    - GHSA-ignr-1111-2222\n",
    );

    let output = pacquet.arg("audit").arg("--json").output().expect("run pacquet audit");

    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    let advisories = report["advisories"].as_object().unwrap();
    assert!(!advisories.contains_key("401"));
    assert_eq!(advisories["402"]["title"], "visible vulnerability");
    assert_eq!(report["metadata"]["vulnerabilities"]["high"], 1);
    mock.assert();
}

#[test]
fn audit_level_info_includes_info_advisories() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        &advisory_response("info-pkg", 501, "info", "*", "just some info", "GHSA-info-info-info"),
    )
    .create();
    write_audit_workspace(&workspace, &registry.url(), "");

    let output =
        pacquet.arg("audit").arg("--audit-level").arg("info").output().expect("run pacquet audit");

    assert_eq!(output.status.code(), Some(1));
    let stdout = stdout(&output);
    assert!(stdout.contains("just some info"));
    assert!(stdout.contains("info"));
    mock.assert();
}

#[test]
fn audit_defaults_to_low_and_ignores_info_for_exit_code() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        &advisory_response("info-pkg", 502, "info", "*", "just some info", "GHSA-info-info-info"),
    )
    .create();
    write_audit_workspace(&workspace, &registry.url(), "");

    let output = pacquet.arg("audit").output().expect("run pacquet audit");

    assert_success(&output);
    assert_eq!(stdout(&output), "1 vulnerabilities found\nSeverity: 1 info");
    mock.assert();
}

#[test]
fn audit_signatures_is_reported_as_unsupported() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    write_minimal_manifest(&workspace);

    let output = pacquet.arg("audit").arg("signatures").output().expect("run pacquet audit");

    assert_failure(&output);
    assert!(stderr(&output).contains("not supported yet"));
}

#[test]
fn audit_rejects_unknown_subcommands() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    write_minimal_manifest(&workspace);

    let output = pacquet.arg("audit").arg("unknown").output().expect("run pacquet audit");

    assert_failure(&output);
    assert!(stderr(&output).contains("ERR_PNPM_AUDIT_UNKNOWN_SUBCOMMAND"));
    assert!(stderr(&output).contains("Unknown audit subcommand: unknown"));
}

#[test]
fn audit_fix_options_are_reported_as_unsupported() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    write_minimal_manifest(&workspace);

    let output = pacquet.arg("audit").arg("--fix").output().expect("run pacquet audit");

    assert_failure(&output);
    assert!(stderr(&output).contains("unexpected argument '--fix'"));
}

#[test]
fn audit_ignore_options_are_reported_as_unsupported() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    write_minimal_manifest(&workspace);

    let output = pacquet.arg("audit").arg("--ignore").arg("GHSA-test-1111-2222").output().unwrap();

    assert_failure(&output);
    assert!(stderr(&output).contains("unexpected argument '--ignore'"));
}

#[test]
fn audit_ignore_unfixable_options_are_reported_as_unsupported() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    write_minimal_manifest(&workspace);

    let output = pacquet.arg("audit").arg("--ignore-unfixable").output().unwrap();

    assert_failure(&output);
    assert!(stderr(&output).contains("unexpected argument '--ignore-unfixable'"));
}

#[test]
fn audit_interactive_fix_options_are_reported_as_unsupported() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    write_minimal_manifest(&workspace);

    let output = pacquet.arg("audit").arg("--fix").arg("--interactive").output().unwrap();

    assert_failure(&output);
    assert!(stderr(&output).contains("unexpected argument '--fix'"));
}

fn audit_mock(registry: &mut mockito::Server, body: &str) -> mockito::Mock {
    registry
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
}

fn advisory_response(
    package: &str,
    id: u64,
    severity: &str,
    vulnerable_versions: &str,
    title: &str,
    ghsa: &str,
) -> String {
    format!(
        r#"{{
  "{package}": [
    {{
      "id": {id},
      "url": "https://github.com/advisories/{ghsa}",
      "title": "{title}",
      "severity": "{severity}",
      "vulnerable_versions": "{vulnerable_versions}",
      "cwe": []
    }}
  ]
}}"#,
    )
}

fn write_audit_workspace(workspace: &Path, registry_url: &str, workspace_yaml: &str) {
    write_audit_workspace_with_npmrc(
        workspace,
        &format!("registry={registry_url}/\n"),
        workspace_yaml,
    );
}

fn write_audit_workspace_with_npmrc(workspace: &Path, npmrc: &str, workspace_yaml: &str) {
    fs::write(workspace.join(".npmrc"), npmrc).expect("write .npmrc");
    fs::write(workspace.join("pnpm-workspace.yaml"), format!("fetchRetries: 0\n{workspace_yaml}"))
        .expect("write workspace manifest");
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"audit-test","version":"1.0.0","dependencies":{"vulnerable":"1.0.0","moderate-pkg":"1.0.0","info-pkg":"1.0.0"},"devDependencies":{"dev-vulnerable":"1.0.0"},"optionalDependencies":{"optional-vulnerable":"1.0.0"}}"#,
    )
    .expect("write package.json");
    fs::write(
        workspace.join("pnpm-lock.yaml"),
        r"
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      vulnerable:
        specifier: '1.0.0'
        version: '1.0.0'
      moderate-pkg:
        specifier: '1.0.0'
        version: '1.0.0'
      info-pkg:
        specifier: '1.0.0'
        version: '1.0.0'
    devDependencies:
      dev-vulnerable:
        specifier: '1.0.0'
        version: '1.0.0'
    optionalDependencies:
      optional-vulnerable:
        specifier: '1.0.0'
        version: '1.0.0'

snapshots:

  vulnerable@1.0.0: {}

  moderate-pkg@1.0.0: {}

  info-pkg@1.0.0: {}

  dev-vulnerable@1.0.0: {}

  optional-vulnerable@1.0.0: {}
",
    )
    .expect("write lockfile");
}

fn write_minimal_manifest(workspace: &Path) {
    fs::write(workspace.join("package.json"), r#"{"name":"audit-test","version":"1.0.0"}"#)
        .expect("write package.json");
}

fn nerf(registry_url: &str) -> &str {
    registry_url.strip_prefix("http:").expect("mockito registry uses http")
}

fn assert_success(output: &Output) {
    assert!(output.status.success(), "stderr:\n{}", stderr(output));
}

fn assert_failure(output: &Output) {
    assert!(!output.status.success(), "stdout:\n{}", stdout(output));
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
