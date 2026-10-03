use super::{CommandTempCwd, advisory_response, assert_failure, fs, pacquet_cmd, stderr, stdout};
use mockito::Matcher;
use std::path::Path;

/// Point the default registry, which serves the bulk advisory endpoint, at
/// `audit_registry`, and resolve the `@pnpm.e2e` fixtures from pnpr.
fn write_npmrc(workspace: &Path, audit_registry: &str, pnpr: &str) {
    fs::write(
        workspace.join(".npmrc"),
        format!(
            "registry={audit_registry}\n@pnpm.e2e:registry={pnpr}\nstore-dir=../pacquet-store\ncache-dir=../pacquet-cache\nfetchRetries=0\n",
        ),
    )
    .expect("write .npmrc");
}

#[test]
fn audit_package_audits_its_dependencies_without_a_project() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let mut audit_registry = mockito::Server::new();
    let mock = audit_registry
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .match_body(Matcher::PartialJsonString(
            r#"{"@pnpm.e2e/pkg-with-1-dep":["100.0.0"],"@pnpm.e2e/dep-of-pkg-with-1-dep":["100.1.0"]}"#
                .to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(advisory_response(
            "@pnpm.e2e/dep-of-pkg-with-1-dep",
            9101,
            "high",
            "<101.0.0",
            "vulnerable dependency",
            "GHSA-pkgs-1111-2222",
        ))
        .create();
    write_npmrc(&workspace, &audit_registry.url(), npmrc_info.mock_instance.url());
    let workspace_entries = || {
        let mut names: Vec<_> = fs::read_dir(&workspace)
            .expect("read workspace")
            .map(|entry| entry.expect("workspace entry").file_name())
            .collect();
        names.sort();
        names
    };
    let entries_before = workspace_entries();

    let output = pacquet_cmd(&workspace, ["audit", "@pnpm.e2e/pkg-with-1-dep@100.0.0", "--json"])
        .output()
        .expect("run pacquet audit");

    assert_eq!(output.status.code(), Some(1), "stderr:\n{}", stderr(&output));
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("audit JSON");
    assert_eq!(
        report["advisories"]["9101"]["findings"][0]["paths"][0],
        ".>@pnpm.e2e/pkg-with-1-dep>@pnpm.e2e/dep-of-pkg-with-1-dep",
    );
    assert_eq!(workspace_entries(), entries_before, "audit wrote to the working directory");
    mock.assert();
    drop((root, npmrc_info));
}

#[test]
fn audit_package_audits_the_version_its_range_resolves_to() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let mut audit_registry = mockito::Server::new();
    let mock = audit_registry
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .match_body(Matcher::Json(serde_json::json!({
            "@pnpm.e2e/audit-multi-version": ["2.0.0"],
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(advisory_response(
            "@pnpm.e2e/audit-multi-version",
            9102,
            "high",
            "=2.0.0",
            "vulnerable 2.0.0",
            "GHSA-rang-1111-2222",
        ))
        .create();
    write_npmrc(&workspace, &audit_registry.url(), npmrc_info.mock_instance.url());

    let output = pacquet_cmd(&workspace, ["audit", "@pnpm.e2e/audit-multi-version@<2.0.1"])
        .output()
        .expect("run pacquet audit");

    assert_eq!(output.status.code(), Some(1), "stderr:\n{}", stderr(&output));
    assert!(stdout(&output).contains("vulnerable 2.0.0"), "stdout:\n{}", stdout(&output));
    mock.assert();
    drop((root, npmrc_info));
}

#[test]
fn audit_package_prod_takes_precedence_over_dev() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let mut audit_registry = mockito::Server::new();
    let mock = audit_registry
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .match_body(Matcher::Json(serde_json::json!({
            "@pnpm.e2e/audit-multi-version": ["2.0.0"],
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(advisory_response(
            "@pnpm.e2e/audit-multi-version",
            9104,
            "high",
            "=2.0.0",
            "vulnerable 2.0.0",
            "GHSA-prod-1111-2222",
        ))
        .create();
    write_npmrc(&workspace, &audit_registry.url(), npmrc_info.mock_instance.url());

    let output = pacquet_cmd(
        &workspace,
        ["audit", "@pnpm.e2e/audit-multi-version@2.0.0", "--prod", "--dev", "--json"],
    )
    .output()
    .expect("run pacquet audit");

    assert_eq!(output.status.code(), Some(1), "stderr:\n{}", stderr(&output));
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("audit JSON");
    assert_eq!(report["advisories"]["9104"]["findings"][0]["dev"], false);
    mock.assert();
    drop((root, npmrc_info));
}

#[test]
fn audit_package_dev_audits_the_package_as_a_dev_dependency() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let mut audit_registry = mockito::Server::new();
    let mock = audit_registry
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .match_body(Matcher::Json(serde_json::json!({
            "@pnpm.e2e/audit-multi-version": ["2.0.0"],
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(advisory_response(
            "@pnpm.e2e/audit-multi-version",
            9103,
            "high",
            "=2.0.0",
            "vulnerable 2.0.0",
            "GHSA-devp-1111-2222",
        ))
        .create();
    write_npmrc(&workspace, &audit_registry.url(), npmrc_info.mock_instance.url());

    let output = pacquet_cmd(
        &workspace,
        ["audit", "@pnpm.e2e/audit-multi-version@2.0.0", "--dev", "--json"],
    )
    .output()
    .expect("run pacquet audit");

    assert_eq!(output.status.code(), Some(1), "stderr:\n{}", stderr(&output));
    let report: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("audit JSON");
    assert_eq!(report["advisories"]["9103"]["findings"][0]["dev"], true);
    mock.assert();
    drop((root, npmrc_info));
}

#[test]
fn audit_package_rejects_options_that_change_a_project() {
    let CommandTempCwd { workspace, root: _root, .. } = CommandTempCwd::init();

    let output =
        pacquet_cmd(&workspace, ["audit", "lodash", "--fix"]).output().expect("run pacquet audit");

    assert_failure(&output);
    assert!(
        stderr(&output).contains("ERR_PNPM_AUDIT_PACKAGES_WITH_PROJECT_OPTION"),
        "stderr:\n{}",
        stderr(&output),
    );
}

#[test]
fn audit_package_rejects_a_package_named_twice() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let audit_registry = mockito::Server::new();
    write_npmrc(&workspace, &audit_registry.url(), npmrc_info.mock_instance.url());

    let output = pacquet_cmd(
        &workspace,
        ["audit", "@pnpm.e2e/audit-multi-version@1.0.0", "@pnpm.e2e/audit-multi-version@2.0.0"],
    )
    .output()
    .expect("run pacquet audit");

    assert_failure(&output);
    assert!(
        stderr(&output).contains("ERR_PNPM_AUDIT_DUPLICATE_PACKAGE"),
        "stderr:\n{}",
        stderr(&output),
    );
    drop((root, npmrc_info));
}
