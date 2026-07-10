use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use mockito::Matcher;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{
    ffi::OsStr,
    fs,
    path::Path,
    process::{Command, Output},
};

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
fn audit_signatures_reports_verified_packages() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let key = signing_key();
    let integrity = "sha512-abc";
    let signature = sign_b64(&key, &format!("signed-pkg@1.0.0:{integrity}"));
    let keys_mock = keys_mock(&mut registry, &public_key_b64(&key)).create();
    let packument_mock = registry
        .mock("GET", "/signed-pkg")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument_body("signed-pkg", "1.0.0", integrity, &signatures_json(&signature)))
        .create();
    write_signatures_workspace(&workspace, &registry.url(), "signed-pkg");

    let output = pacquet.arg("audit").arg("signatures").output().expect("run audit signatures");

    assert_success(&output);
    let out = stdout(&output);
    assert!(out.contains("audited 1 package"), "{out}");
    assert!(out.contains("1 package has a verified registry signature"), "{out}");
    keys_mock.assert();
    packument_mock.assert();
}

#[test]
fn audit_signatures_json_reports_counts() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let key = signing_key();
    let integrity = "sha512-abc";
    let signature = sign_b64(&key, &format!("signed-pkg@1.0.0:{integrity}"));
    let keys_mock = keys_mock(&mut registry, &public_key_b64(&key)).create();
    let packument_mock = registry
        .mock("GET", "/signed-pkg")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument_body("signed-pkg", "1.0.0", integrity, &signatures_json(&signature)))
        .create();
    write_signatures_workspace(&workspace, &registry.url(), "signed-pkg");

    let output = pacquet
        .arg("audit")
        .arg("signatures")
        .arg("--json")
        .output()
        .expect("run audit signatures");

    assert_success(&output);
    let report: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("signatures JSON");
    assert_eq!(report["audited"], 1);
    assert_eq!(report["verified"], 1);
    assert_eq!(report["invalid"].as_array().expect("invalid array").len(), 0);
    assert_eq!(report["missing"].as_array().expect("missing array").len(), 0);
    keys_mock.assert();
    packument_mock.assert();
}

#[test]
fn audit_signatures_flags_missing_signature() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let key = signing_key();
    let keys_mock = keys_mock(&mut registry, &public_key_b64(&key)).create();
    let packument_mock = registry
        .mock("GET", "/signed-pkg")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument_body("signed-pkg", "1.0.0", "sha512-abc", "[]"))
        .create();
    write_signatures_workspace(&workspace, &registry.url(), "signed-pkg");

    let output = pacquet.arg("audit").arg("signatures").output().expect("run audit signatures");

    assert_eq!(output.status.code(), Some(1), "missing signatures should exit 1");
    let out = stdout(&output);
    assert!(out.contains("missing registry signature"), "{out}");
    assert!(out.contains("signed-pkg@1.0.0"), "{out}");
    keys_mock.assert();
    packument_mock.assert();
}

#[test]
fn audit_signatures_flags_invalid_signature() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let key = signing_key();
    // Sign a different integrity than the packument advertises: the signature
    // is well-formed but will not validate over the published bytes.
    let signature = sign_b64(&key, "signed-pkg@1.0.0:sha512-tampered");
    let keys_mock = keys_mock(&mut registry, &public_key_b64(&key)).create();
    let packument_mock = registry
        .mock("GET", "/signed-pkg")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument_body(
            "signed-pkg",
            "1.0.0",
            "sha512-abc",
            &signatures_json(&signature),
        ))
        .create();
    write_signatures_workspace(&workspace, &registry.url(), "signed-pkg");

    let output = pacquet.arg("audit").arg("signatures").output().expect("run audit signatures");

    assert_eq!(output.status.code(), Some(1), "invalid signatures should exit 1");
    let out = stdout(&output);
    assert!(out.contains("invalid registry signature"), "{out}");
    assert!(out.contains("Someone might have tampered"), "{out}");
    keys_mock.assert();
    packument_mock.assert();
}

#[test]
fn audit_signatures_skips_registry_without_signing_keys() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let keys_mock =
        registry.mock("GET", "/-/npm/v1/keys").with_status(404).with_body("not found").create();
    write_signatures_workspace(&workspace, &registry.url(), "signed-pkg");

    let output = pacquet.arg("audit").arg("signatures").output().expect("run audit signatures");

    assert_success(&output);
    let out = stdout(&output);
    assert!(out.contains("audited 0 packages"), "{out}");
    assert!(
        out.contains("No dependencies were installed from a registry with signing keys"),
        "{out}",
    );
    keys_mock.assert();
}

#[test]
fn audit_signatures_fails_when_keys_endpoint_errors() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let keys_mock = registry
        .mock("GET", "/-/npm/v1/keys")
        .with_status(500)
        .with_body("boom \u{1b}[31m\n")
        .create();
    write_signatures_workspace(&workspace, &registry.url(), "signed-pkg");

    let output = pacquet.arg("audit").arg("signatures").output().expect("run audit signatures");

    assert_failure(&output);
    let stderr = stderr(&output);
    assert!(stderr.contains("ERR_PNPM_AUDIT_SIGNATURE_KEYS_FETCH_FAIL"), "stderr:\n{stderr}");
    assert!(stderr.contains("responded with 500"), "stderr:\n{stderr}");
    // The attacker-controlled registry body is escaped before it reaches the
    // terminal: the raw ESC byte must not survive.
    assert!(stderr.contains(r"boom \u{1b}[31m\u{a}"), "stderr:\n{stderr}");
    assert!(!stderr.contains('\u{1b}'), "stderr:\n{stderr}");
    keys_mock.assert();
}

#[test]
fn audit_signatures_redacts_registry_credentials_on_network_error() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    // A registry with embedded credentials pointed at a closed port: the keys
    // fetch fails at the transport layer, and the resulting error must not leak
    // the `user:pass@` userinfo into stderr.
    write_signatures_workspace(&workspace, "https://user:pass@127.0.0.1:1", "signed-pkg");

    let output = pacquet.arg("audit").arg("signatures").output().expect("run audit signatures");

    assert_failure(&output);
    let stderr = stderr(&output);
    assert!(stderr.contains("ERR_PNPM_AUDIT_SIGNATURE_KEYS_FETCH_FAIL"), "stderr:\n{stderr}");
    assert!(!stderr.contains("user:pass"), "credentials leaked into stderr:\n{stderr}");
    assert!(!stderr.contains("pass@"), "credentials leaked into stderr:\n{stderr}");
}

#[test]
fn audit_signatures_errors_when_no_packages() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    fs::write(workspace.join(".npmrc"), "registry=https://registry.npmjs.org/\n")
        .expect("write .npmrc");
    fs::write(workspace.join("pnpm-workspace.yaml"), "fetchRetries: 0\n")
        .expect("write workspace manifest");
    write_minimal_manifest(&workspace);
    fs::write(
        workspace.join("pnpm-lock.yaml"),
        "
lockfileVersion: '9.0'

importers:

  .: {}
",
    )
    .expect("write lockfile");

    let output = pacquet.arg("audit").arg("signatures").output().expect("run audit signatures");

    assert_failure(&output);
    assert!(stderr(&output).contains("ERR_PNPM_AUDIT_NO_PACKAGES"), "stderr:\n{}", stderr(&output));
}

#[test]
fn audit_signatures_rejects_extra_subcommand_argument() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    write_minimal_manifest(&workspace);

    let output =
        pacquet.arg("audit").arg("signatures").arg("extra").output().expect("run pacquet audit");

    assert_failure(&output);
    assert!(stderr(&output).contains("ERR_PNPM_AUDIT_UNKNOWN_SUBCOMMAND"));
    assert!(stderr(&output).contains("Unknown audit subcommand: signatures extra"));
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
fn audit_fix_override_writes_overrides_to_workspace_manifest() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        &advisory_response("vulnerable", 123, "high", "<2.0.0", "test", "GHSA-test-1111-2222"),
    )
    .create();
    write_audit_workspace(&workspace, &registry.url(), "");

    let output = pacquet.arg("audit").arg("--fix").output().expect("run pacquet audit --fix");

    assert_success(&output);
    assert!(stdout(&output).contains("overrides were added to pnpm-workspace.yaml"));
    let manifest =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace manifest");
    assert!(
        manifest.contains("overrides:") && manifest.contains("vulnerable@<2.0.0: ^2.0.0"),
        "manifest should hold the override:\n{manifest}",
    );
    mock.assert();
}

#[test]
fn audit_fix_override_writes_minimum_release_age_excludes_when_configured() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        &advisory_response("vulnerable", 123, "high", "<2.0.0", "test", "GHSA-test-1111-2222"),
    )
    .create();
    write_audit_workspace(&workspace, &registry.url(), "minimumReleaseAge: 1440\n");

    let output = pacquet.arg("audit").arg("--fix").output().expect("run pacquet audit --fix");

    assert_success(&output);
    assert!(
        stdout(&output).contains("entries were added to minimumReleaseAgeExclude"),
        "stdout should report the exclusions:\n{}",
        stdout(&output),
    );
    let manifest =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace manifest");
    assert!(
        manifest.contains("minimumReleaseAgeExclude:") && manifest.contains("- vulnerable@2.0.0"),
        "manifest should hold the patched-version exclusion:\n{manifest}",
    );
    mock.assert();
}

#[test]
fn audit_fix_override_with_no_fixable_vulnerabilities_makes_no_changes() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    // `>=0.0.0` has no inferable patched range, so no override is possible.
    let mock = audit_mock(
        &mut registry,
        &advisory_response("vulnerable", 123, "high", ">=0.0.0", "test", "GHSA-test-1111-2222"),
    )
    .create();
    write_audit_workspace(&workspace, &registry.url(), "");

    let output = pacquet.arg("audit").arg("--fix").output().expect("run pacquet audit --fix");

    assert_success(&output);
    assert_eq!(stdout(&output), "No fixes were made");
    mock.assert();
}

#[test]
fn audit_fix_rejects_invalid_method() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(&mut registry, "{}").expect(0).create();
    write_audit_workspace(&workspace, &registry.url(), "");

    let output =
        pacquet.arg("audit").arg("--fix").arg("nonsense").output().expect("run pacquet audit");

    assert_failure(&output);
    assert!(stderr(&output).contains("Invalid value for --fix: nonsense"));
    mock.assert();
}

#[test]
fn audit_ignore_writes_ghsa_to_audit_config() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        &advisory_response("vulnerable", 123, "high", "<2.0.0", "test", "GHSA-test-1111-2222"),
    )
    .create();
    write_audit_workspace(&workspace, &registry.url(), "");

    let output = pacquet
        .arg("audit")
        .arg("--ignore")
        .arg("GHSA-test-1111-2222")
        .output()
        .expect("run pacquet audit --ignore");

    assert_success(&output);
    assert!(stdout(&output).contains("1 new vulnerabilities were ignored"));
    assert!(stdout(&output).contains("GHSA-test-1111-2222"));
    let manifest =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace manifest");
    assert!(
        manifest.contains("auditConfig:") && manifest.contains("- GHSA-test-1111-2222"),
        "manifest should hold the ignored GHSA:\n{manifest}",
    );
    mock.assert();
}

#[test]
fn audit_ignore_unfixable_ignores_advisories_without_a_fix() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    // `>=0.0.0` is unfixable (no inferable patched range).
    let mock = audit_mock(
        &mut registry,
        &advisory_response("vulnerable", 123, "high", ">=0.0.0", "test", "GHSA-test-1111-2222"),
    )
    .create();
    write_audit_workspace(&workspace, &registry.url(), "");

    let output = pacquet
        .arg("audit")
        .arg("--ignore-unfixable")
        .output()
        .expect("run pacquet audit --ignore-unfixable");

    assert_success(&output);
    assert!(stdout(&output).contains("1 new vulnerabilities were ignored"));
    let manifest =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace manifest");
    assert!(
        manifest.contains("- GHSA-test-1111-2222"),
        "manifest should hold the GHSA:\n{manifest}",
    );
    mock.assert();
}

/// Build a fresh single-shot `pacquet` command bound to `workspace`, for
/// multi-step tests (install, then audit) that can't reuse the one-shot
/// command from [`CommandTempCwd`].
fn pacquet_cmd(workspace: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pacquet binary")
        .with_current_dir(workspace)
        .with_args(args)
}

/// End-to-end `audit --fix update`: install a dependency whose range spans a
/// vulnerable high version and a safe low one, mark the high versions
/// vulnerable, and confirm the resolver-time guard walks the lockfile back
/// down to a safe version. The audit advisory endpoint is served by a
/// mockito registry (the default registry); the package itself is resolved
/// from the pnpr fixture registry via a scoped registry.
#[test]
fn audit_fix_update_moves_to_a_non_vulnerable_version() {
    const PKG: &str = "@pnpm.e2e/audit-multi-version";
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpr_url = npmrc_info.mock_instance.url();

    // `>=1.0.0` admits both the vulnerable 2.x and the safe 1.x versions.
    fs::write(
        workspace.join("package.json"),
        format!(
            r#"{{"name":"audit-fix-update","version":"1.0.0","dependencies":{{"{PKG}":">=1.0.0"}}}}"#,
        ),
    )
    .expect("write package.json");

    // Install against the pnpr fixture registry (the default registry the
    // harness wrote). The highest in-range version, 2.0.1, is installed.
    pacquet_cmd(&workspace, ["install"]).assert().success();
    assert!(
        workspace.join("node_modules/.pnpm").join("@pnpm.e2e+audit-multi-version@2.0.1").exists(),
        "install should pick the highest in-range version",
    );

    // Serve the audit advisory marking every 2.x version vulnerable.
    let mut audit_registry = mockito::Server::new();
    let mock = audit_registry
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(advisory_response(
            PKG,
            9001,
            "high",
            ">=2.0.0",
            "vulnerable 2.x",
            "GHSA-mult-1111-2222",
        ))
        .create();

    // Default registry → audit mock (serves the bulk endpoint); the scoped
    // package keeps resolving from pnpr.
    fs::write(
        workspace.join(".npmrc"),
        format!(
            "registry={audit}\n@pnpm.e2e:registry={pnpr}\nstore-dir=../pacquet-store\ncache-dir=../pacquet-cache\nfetchRetries=0\n",
            audit = audit_registry.url(),
            pnpr = pnpr_url,
        ),
    )
    .expect("rewrite .npmrc");

    let output = pacquet_cmd(&workspace, ["audit", "--fix", "update"])
        .output()
        .expect("run audit --fix update");

    assert!(
        output.status.success(),
        "audit --fix update should succeed; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("vulnerability was fixed"), "stdout should report the fix:\n{stdout}");

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile.contains("audit-multi-version@1.0.1"),
        "the guard should move resolution to the safe 1.0.1:\n{lockfile}",
    );
    assert!(
        !lockfile.contains("audit-multi-version@2.0."),
        "no vulnerable 2.x version should remain:\n{lockfile}",
    );
    mock.assert();
    drop((root, npmrc_info));
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

const SIGNATURE_KEYID: &str = "SHA256:test";

fn signing_key() -> p256::ecdsa::SigningKey {
    p256::ecdsa::SigningKey::from_slice(&[0x42; 32]).expect("valid P-256 scalar")
}

fn public_key_b64(key: &p256::ecdsa::SigningKey) -> String {
    use base64::Engine as _;
    use p256::pkcs8::EncodePublicKey;
    let der = key.verifying_key().to_public_key_der().expect("encode SPKI");
    base64::engine::general_purpose::STANDARD.encode(der.as_bytes())
}

fn sign_b64(key: &p256::ecdsa::SigningKey, message: &str) -> String {
    use base64::Engine as _;
    use p256::ecdsa::{Signature, signature::Signer};
    let signature: Signature = key.sign(message.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(signature.to_der().as_bytes())
}

fn keys_mock(registry: &mut mockito::Server, public_key_b64: &str) -> mockito::Mock {
    registry
        .mock("GET", "/-/npm/v1/keys")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"keys":[{{"expires":null,"keyid":"{SIGNATURE_KEYID}","keytype":"ecdsa-sha2-nistp256","scheme":"ecdsa-sha2-nistp256","key":"{public_key_b64}"}}]}}"#,
        ))
}

fn signatures_json(signature_b64: &str) -> String {
    format!(r#"[{{"keyid":"{SIGNATURE_KEYID}","sig":"{signature_b64}"}}]"#)
}

fn packument_body(name: &str, version: &str, integrity: &str, signatures_json: &str) -> String {
    format!(
        r#"{{"name":"{name}","versions":{{"{version}":{{"name":"{name}","version":"{version}","dist":{{"integrity":"{integrity}","tarball":"https://example.com/{name}-{version}.tgz","signatures":{signatures_json}}}}}}},"time":{{"{version}":"2020-01-01T00:00:00.000Z"}}}}"#,
    )
}

fn write_signatures_workspace(workspace: &Path, registry_url: &str, name: &str) {
    fs::write(workspace.join(".npmrc"), format!("registry={registry_url}/\n"))
        .expect("write .npmrc");
    fs::write(workspace.join("pnpm-workspace.yaml"), "fetchRetries: 0\n")
        .expect("write workspace manifest");
    fs::write(
        workspace.join("package.json"),
        format!(r#"{{"name":"sig-test","version":"1.0.0","dependencies":{{"{name}":"1.0.0"}}}}"#),
    )
    .expect("write package.json");
    fs::write(
        workspace.join("pnpm-lock.yaml"),
        format!(
            "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      {name}:
        specifier: '1.0.0'
        version: '1.0.0'

snapshots:

  {name}@1.0.0: {{}}
",
        ),
    )
    .expect("write lockfile");
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
