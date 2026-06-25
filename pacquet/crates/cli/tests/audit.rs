use pacquet_testing_utils::bin::CommandTempCwd;
use std::fs;

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

    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", registry.url()))
        .expect("write .npmrc");
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"audit-test","version":"1.0.0","dependencies":{"vulnerable":"1.0.0"}}"#,
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

snapshots:

  vulnerable@1.0.0: {}
",
    )
    .expect("write lockfile");

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
