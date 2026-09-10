use super::{
    CommandTempCwd, advisory_entry, advisory_response, assert_failure, assert_success,
    audit_config_ignore_ghsas, audit_mock, fs, pacquet_cmd, set_minimum_release_age, stderr,
    stdout, write_audit_workspace,
};
use assert_cmd::assert::OutputAssertExt;

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
    let out = stdout(&output);
    assert!(out.contains("overrides were added to pnpm-workspace.yaml"), "{out}");
    assert!(out.ends_with('\n'), "fix output should end with a newline:\n{out}");
    let manifest =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace manifest");
    assert!(
        manifest.contains("overrides:") && manifest.contains("vulnerable@<2.0.0: ^2.0.0"),
        "manifest should hold the override:\n{manifest}",
    );
    mock.assert();
}

#[test]
fn audit_fix_override_writes_overrides_in_the_configured_save_style() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        &advisory_response("vulnerable", 123, "high", "<2.0.0", "test", "GHSA-test-1111-2222"),
    )
    .create();
    write_audit_workspace(&workspace, &registry.url(), "savePrefix: '~'\n");

    let output = pacquet.arg("audit").arg("--fix").output().expect("run pacquet audit --fix");

    assert_success(&output);
    let manifest =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace manifest");
    assert!(
        manifest.contains("vulnerable@<2.0.0: ~2.0.0"),
        "manifest should hold the tilde override:\n{manifest}",
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
fn audit_fix_override_skips_age_exclude_when_patched_version_is_old() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        &advisory_response("vulnerable", 123, "high", "<2.0.0", "test", "GHSA-test-1111-2222"),
    )
    .create();
    // The patched version predates the cutoff, so the age gate would not
    // block it and no exclusion entry is needed. The packument is fetched
    // once: validation and the age-gate check share the same publish-time map.
    let packument_mock = registry
        .mock("GET", "/vulnerable")
        .expect(1)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"time":{"2.0.0":"2020-01-01T00:00:00.000Z"}}"#)
        .create();
    write_audit_workspace(&workspace, &registry.url(), "minimumReleaseAge: 1440\n");

    let output = pacquet.arg("audit").arg("--fix").output().expect("run pacquet audit --fix");

    assert_success(&output);
    assert!(stdout(&output).contains("overrides were added to pnpm-workspace.yaml"));
    assert!(
        !stdout(&output).contains("entries were added to minimumReleaseAgeExclude"),
        "no exclusions should be reported:\n{}",
        stdout(&output),
    );
    let manifest =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace manifest");
    assert!(manifest.contains("overrides:"), "manifest should hold the override:\n{manifest}");
    assert!(
        manifest.contains("vulnerable@<2.0.0: ^2.0.0"),
        "manifest should hold the patched override:\n{manifest}",
    );
    assert!(
        !manifest.contains("minimumReleaseAgeExclude:"),
        "manifest should hold no exclusion:\n{manifest}",
    );
    mock.assert();
    packument_mock.assert();
}

#[test]
fn audit_fix_override_makes_no_changes_when_patched_version_is_unpublished() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        &advisory_response("vulnerable", 123, "high", "<2.0.0", "test", "GHSA-test-1111-2222"),
    )
    .create();
    // The packument names no 2.0.0: the patched release was never published,
    // so the inferred range is dropped and nothing is written.
    let packument_mock = registry
        .mock("GET", "/vulnerable")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"time":{"1.0.0":"2020-01-01T00:00:00.000Z"}}"#)
        .create();
    write_audit_workspace(&workspace, &registry.url(), "minimumReleaseAge: 1440\n");

    let output = pacquet.arg("audit").arg("--fix").output().expect("run pacquet audit --fix");

    assert_success(&output);
    assert!(
        stdout(&output).contains("No fixes were made"),
        "an unpublished patch has no fix to apply:\n{}",
        stdout(&output),
    );
    let manifest =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace manifest");
    assert!(!manifest.contains("overrides:"), "manifest should hold no override:\n{manifest}");
    assert!(
        !manifest.contains("minimumReleaseAgeExclude:"),
        "manifest should hold no exclusion:\n{manifest}",
    );
    mock.assert();
    packument_mock.assert();
}

#[test]
fn audit_fix_override_writes_age_exclude_when_patched_version_is_within_the_window() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    // Two advisories on one package: the packument is fetched once and shared
    // between validation and the age-gate check.
    let mock = audit_mock(
        &mut registry,
        r#"{
  "vulnerable": [
    {
      "id": 123,
      "url": "https://github.com/advisories/GHSA-test-1111-2222",
      "title": "test",
      "severity": "high",
      "vulnerable_versions": "<2.0.0",
      "cwe": []
    },
    {
      "id": 124,
      "url": "https://github.com/advisories/GHSA-test-3333-4444",
      "title": "test",
      "severity": "high",
      "vulnerable_versions": "<1.5.0",
      "cwe": []
    }
  ]
}"#,
    )
    .create();
    let packument_mock = registry
        .mock("GET", "/vulnerable")
        .expect(1)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"time":{"1.5.0":"2020-01-01T00:00:00.000Z","2.0.0":"2020-01-01T00:00:00.000Z"}}"#,
        )
        .create();
    // A ~19-year window puts the 2020 publish times after the cutoff, so the
    // age gate would block the patched versions and the exclusions stay.
    write_audit_workspace(&workspace, &registry.url(), "minimumReleaseAge: 10000000\n");

    let output = pacquet.arg("audit").arg("--fix").output().expect("run pacquet audit --fix");

    assert_success(&output);
    assert!(
        stdout(&output).contains("entries were added to minimumReleaseAgeExclude"),
        "exclusions should be reported:\n{}",
        stdout(&output),
    );
    let manifest =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace manifest");
    assert!(
        manifest.contains("minimumReleaseAgeExclude:"),
        "manifest should hold the exclusions:\n{manifest}",
    );
    assert!(
        manifest.contains("vulnerable@1.5.0 || 2.0.0"),
        "manifest should hold both patched-version exclusions:\n{manifest}",
    );
    mock.assert();
    packument_mock.assert();
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
    assert_eq!(stdout(&output), "No fixes were made\n");
    mock.assert();
}

#[test]
fn audit_fix_ignore_prune_removes_unused_ignored_ghsas() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    // GHSA-test-1111-2222 exists in the report; GHSA-test-9999-9999 doesn't.
    let mock = audit_mock(
        &mut registry,
        &advisory_response("vulnerable", 123, "high", "<2.0.0", "test", "GHSA-test-1111-2222"),
    )
    .create();
    write_audit_workspace(
        &workspace,
        &registry.url(),
        "audit:\n  ignorePrune: true\nauditConfig:\n  ignoreGhsas:\n    - GHSA-test-1111-2222\n    - GHSA-test-9999-9999\n",
    );

    let output = pacquet.arg("audit").arg("--fix").output().expect("run pacquet audit --fix");

    assert_success(&output);
    assert!(
        stdout(&output).contains("Removed 1 unused ignored GHSA: GHSA-test-9999-9999"),
        "stdout should report the removed GHSA:\n{}",
        stdout(&output),
    );
    let manifest =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace manifest");
    assert!(
        manifest.contains("GHSA-test-1111-2222") && !manifest.contains("GHSA-test-9999-9999"),
        "manifest should retain the still-relevant GHSA and drop the unused one:\n{manifest}",
    );
    mock.assert();
}

#[test]
fn audit_fix_ignore_prune_disabled_by_default_keeps_all_ignored_ghsas() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        &advisory_response("vulnerable", 123, "high", "<2.0.0", "test", "GHSA-test-1111-2222"),
    )
    .create();
    write_audit_workspace(
        &workspace,
        &registry.url(),
        "auditConfig:\n  ignoreGhsas:\n    - GHSA-test-1111-2222\n    - GHSA-test-9999-9999\n",
    );

    let output = pacquet.arg("audit").arg("--fix").output().expect("run pacquet audit --fix");

    assert_success(&output);
    assert!(!stdout(&output).contains("unused ignored GHSA"));
    let manifest =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace manifest");
    assert!(
        manifest.contains("GHSA-test-9999-9999"),
        "manifest should keep the unused GHSA since pruning is disabled:\n{manifest}",
    );
    mock.assert();
}

#[test]
fn audit_fix_ignore_prune_normalizes_ghsa_casing() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        &advisory_response("vulnerable", 123, "high", "<2.0.0", "test", "GHSA-test-1111-2222"),
    )
    .create();
    write_audit_workspace(
        &workspace,
        &registry.url(),
        "audit:\n  ignorePrune: true\nauditConfig:\n  ignoreGhsas:\n    - ghsa-test-1111-2222\n    - GHSA-TEST-9999-9999\n",
    );

    let output = pacquet.arg("audit").arg("--fix").output().expect("run pacquet audit --fix");

    assert_success(&output);
    // Retained entries are rewritten to their canonical spelling regardless
    // of the casing the user originally ignored them with, and deduplicated
    // — the exact list must be just the one canonical, still-relevant id.
    assert_eq!(audit_config_ignore_ghsas(&workspace), vec!["GHSA-test-1111-2222".to_string()]);
    mock.assert();
}

#[test]
fn audit_fix_ignore_prune_persists_canonical_form_even_when_nothing_is_removed() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        &advisory_response("vulnerable", 123, "high", "<2.0.0", "test", "GHSA-test-1111-2222"),
    )
    .create();
    // Both entries match the same advisory (a differently-cased duplicate)
    // — nothing gets removed, but the stored list should still collapse to
    // the single canonical entry.
    write_audit_workspace(
        &workspace,
        &registry.url(),
        "audit:\n  ignorePrune: true\nauditConfig:\n  ignoreGhsas:\n    - ghsa-test-1111-2222\n    - GHSA-TEST-1111-2222\n",
    );

    let output = pacquet.arg("audit").arg("--fix").output().expect("run pacquet audit --fix");

    assert_success(&output);
    assert!(!stdout(&output).contains("unused ignored GHSA"));
    assert_eq!(audit_config_ignore_ghsas(&workspace), vec!["GHSA-test-1111-2222".to_string()]);
    mock.assert();
}

#[test]
fn audit_fix_ignore_prune_removes_a_comment_attached_to_the_removed_entry() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        &advisory_response("vulnerable", 123, "high", "<2.0.0", "test", "GHSA-test-1111-2222"),
    )
    .create();
    write_audit_workspace(
        &workspace,
        &registry.url(),
        "audit:\n  ignorePrune: true\nauditConfig:\n  ignoreGhsas:\n    - GHSA-test-1111-2222\n    # Expired GHSA, should not be ignored\n    - GHSA-test-9999-9999 # trailing comment, should also go\n",
    );

    let output = pacquet.arg("audit").arg("--fix").output().expect("run pacquet audit --fix");

    assert_success(&output);
    let manifest =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace manifest");
    // Both the preceding comment and the trailing same-line comment attached
    // to the removed entry must go with it.
    assert!(
        !manifest.contains("Expired GHSA") && !manifest.contains("trailing comment"),
        "manifest should drop the comments along with the entry they were attached to:\n{manifest}",
    );
    mock.assert();
}

#[test]
fn audit_fix_ignore_prune_removes_all_when_none_are_relevant() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        &advisory_response("vulnerable", 123, "high", "<2.0.0", "test", "GHSA-test-1111-2222"),
    )
    .create();
    write_audit_workspace(
        &workspace,
        &registry.url(),
        "audit:\n  ignorePrune: true\nauditConfig:\n  ignoreGhsas:\n    - GHSA-test-9999-0001\n    - GHSA-test-9999-0002\n",
    );

    let output = pacquet.arg("audit").arg("--fix").output().expect("run pacquet audit --fix");

    assert_success(&output);
    let manifest =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace manifest");
    assert!(
        !manifest.contains("ignoreGhsas:"),
        "manifest should drop ignoreGhsas once every entry is pruned:\n{manifest}",
    );
    mock.assert();
}

#[test]
fn audit_fix_ignore_prune_edits_an_inline_audit_config_in_place() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        &advisory_response("vulnerable", 123, "high", "<2.0.0", "test", "GHSA-test-1111-2222"),
    )
    .create();
    write_audit_workspace(
        &workspace,
        &registry.url(),
        "audit:\n  ignorePrune: true\nauditConfig: { ignoreGhsas: [GHSA-test-1111-2222, GHSA-test-9999-9999] }\n",
    );

    let output = pacquet.arg("audit").arg("--fix").output().expect("run pacquet audit --fix");

    assert_success(&output);
    let actual_manifest =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace manifest");
    let expected_manifest = "fetchRetries: 0\naudit:\n  ignorePrune: true\nauditConfig: { ignoreGhsas: [ GHSA-test-1111-2222 ] }\n";
    eprintln!("actual manifest:\n{actual_manifest}\nexpected manifest:\n{expected_manifest}");
    assert_eq!(actual_manifest, expected_manifest);
    mock.assert();
}

#[test]
fn audit_fix_ignore_prune_updates_the_canonical_audit_ignore_list() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        &advisory_response("vulnerable", 123, "high", "<2.0.0", "test", "GHSA-test-1111-2222"),
    )
    .create();
    write_audit_workspace(
        &workspace,
        &registry.url(),
        "audit:\n  ignorePrune: true\n  ignore:\n    - GHSA-test-1111-2222\n    - GHSA-test-9999-9999\n",
    );

    let output = pacquet.arg("audit").arg("--fix").output().expect("run pacquet audit --fix");

    assert_success(&output);
    let actual_manifest =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace manifest");
    // The retained list must land back on the canonical `audit.ignore` that
    // supplied it — writing the deprecated `auditConfig.ignoreGhsas` instead
    // would let the unchanged canonical list shadow the prune on the next
    // read and restore the stale id.
    let expected_manifest =
        "fetchRetries: 0\naudit:\n  ignorePrune: true\n  ignore:\n    - GHSA-test-1111-2222\n";
    eprintln!("actual manifest:\n{actual_manifest}\nexpected manifest:\n{expected_manifest}");
    assert_eq!(actual_manifest, expected_manifest);
    mock.assert();
}

#[test]
fn audit_fix_ignore_prune_sanitizes_the_removed_ids_in_output() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mock = audit_mock(
        &mut registry,
        &advisory_response("vulnerable", 123, "high", "<2.0.0", "test", "GHSA-test-1111-2222"),
    )
    .create();
    // The stale entry carries an ANSI escape from the repository-controlled
    // manifest; the removal message must strip it before the terminal.
    write_audit_workspace(
        &workspace,
        &registry.url(),
        "audit:\n  ignorePrune: true\nauditConfig:\n  ignoreGhsas:\n    - GHSA-test-1111-2222\n    - \"GHSA-test-9999-9999\\e[31m\"\n",
    );

    let output = pacquet.arg("audit").arg("--fix").output().expect("run pacquet audit --fix");

    assert_success(&output);
    let out = stdout(&output);
    assert!(
        out.contains("Removed 1 unused ignored GHSA: GHSA-test-9999-9999[31m"),
        "stdout should report the removed GHSA with its control characters stripped:\n{out}",
    );
    assert!(!out.contains('\u{1b}'), "stdout must not carry the escape character:\n{out}");
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
    assert_eq!(stdout(&output), "1 new vulnerabilities were ignored:\nGHSA-test-1111-2222\n");
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

/// A package whose every in-range version is vulnerable has no update that
/// fixes it. The run must still fix what it can elsewhere and report the rest
/// as remaining, rather than failing resolution outright.
#[test]
fn audit_fix_update_keeps_going_when_no_version_in_range_is_safe() {
    const STUCK_PKG: &str = "@pnpm.e2e/audit-multi-version";
    const FIXABLE_PKG: &str = "@pnpm.e2e/multi-version-b";
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpr_url = npmrc_info.mock_instance.url();

    // `^2.0.0` admits only vulnerable 2.x versions of the first package, while
    // `>=1.0.0` leaves the second one a safe 2.0.0 to fall back to.
    fs::write(
        workspace.join("package.json"),
        format!(
            r#"{{"name":"audit-fix-update","version":"1.0.0","dependencies":{{"{STUCK_PKG}":"^2.0.0","{FIXABLE_PKG}":">=1.0.0"}}}}"#,
        ),
    )
    .expect("write package.json");
    pacquet_cmd(&workspace, ["install"]).assert().success();
    assert!(
        workspace.join("node_modules/.pnpm").join("@pnpm.e2e+multi-version-b@3.1.0").exists(),
        "install should pick the highest in-range version",
    );

    let mut audit_registry = mockito::Server::new();
    let body = format!(
        "{{\n{},\n{}\n}}",
        advisory_entry(STUCK_PKG, 9001, "high", ">=2.0.0", "vulnerable 2.x", "GHSA-mult-1111-2222",),
        advisory_entry(
            FIXABLE_PKG,
            9002,
            "high",
            ">=3.0.0",
            "vulnerable 3.x",
            "GHSA-mult-3333-4444",
        ),
    );
    let mock = audit_registry
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create();
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

    // Remaining vulnerabilities still exit 1; what must not happen is the
    // resolver aborting the run.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "stderr:\n{stderr}");
    assert!(stderr.is_empty(), "the run should report no error:\n{stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("1 vulnerability was fixed, 1 vulnerability remains."),
        "stdout should report one fix and one remaining:\n{stdout}",
    );

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile.contains("audit-multi-version@2.0.1"),
        "the vulnerable pick with no safe alternative should stay:\n{lockfile}",
    );
    assert!(
        lockfile.contains("multi-version-b@2.0.0"),
        "the package with a safe version in range should still be updated:\n{lockfile}",
    );
    mock.assert();
    drop((root, npmrc_info));
}

/// `--fix update` with `minimumReleaseAge`: the inferred patched version
/// (3.0.0) has no entry in the packument's `time` map because it was never
/// published, so no exclusion entry is written for it.
#[test]
fn audit_fix_update_skips_age_exclude_when_patched_version_is_unpublished() {
    const PKG: &str = "@pnpm.e2e/audit-multi-version";
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpr_url = npmrc_info.mock_instance.url();

    fs::write(
        workspace.join("package.json"),
        format!(
            r#"{{"name":"audit-fix-update","version":"1.0.0","dependencies":{{"{PKG}":">=1.0.0"}}}}"#,
        ),
    )
    .expect("write package.json");
    pacquet_cmd(&workspace, ["install"]).assert().success();
    set_minimum_release_age(&workspace, 1440);

    let mut audit_registry = mockito::Server::new();
    let mock = audit_registry
        .mock("POST", "/-/npm/v1/security/advisories/bulk")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(advisory_response(
            PKG,
            9001,
            "high",
            ">=2.0.0 <3.0.0",
            "vulnerable 2.x",
            "GHSA-mult-1111-2222",
        ))
        .create();
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
    assert!(
        !stdout.contains("entries were added to minimumReleaseAgeExclude"),
        "no exclusion should be reported:\n{stdout}",
    );
    let manifest =
        fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace manifest");
    assert!(
        !manifest.contains("minimumReleaseAgeExclude:"),
        "manifest should hold no exclusion:\n{manifest}",
    );
    mock.assert();
    drop((root, npmrc_info));
}
