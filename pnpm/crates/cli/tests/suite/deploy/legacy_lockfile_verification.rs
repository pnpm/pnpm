use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, Path, append_workspace_yaml_key,
    deployed_package_version, pacquet_cmd, set_app_foo_dependency, write_workspace,
};
use assert_cmd::assert::OutputAssertExt;

/// A workspace whose lockfile pins `@pnpm.e2e/foo@100.0.0`, then a
/// `minimumReleaseAge` of 100 years that rejects every version the mocked
/// registry serves. Non-strict mode lets the resolver fall back to an
/// immature version, so only the lockfile gate can stop the deploy.
fn write_policy_rejected_workspace(workspace: &Path) {
    write_workspace(workspace, false);
    set_app_foo_dependency(workspace, "100.0.0");
    pacquet_cmd(workspace)
        .with_arg("install")
        .assert()
        .success();
    append_workspace_yaml_key(workspace, "minimumReleaseAge", 60 * 24 * 365 * 100);
    append_workspace_yaml_key(workspace, "minimumReleaseAgeStrict", false);
}

#[test]
fn legacy_deploy_rejects_source_lockfile_entries_that_fail_minimum_release_age() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_policy_rejected_workspace(&workspace);

    let output = pacquet_cmd(&workspace)
        .with_args(["--filter", "app", "deploy", "--legacy", "--prod", "legacy-deploy"])
        .output()
        .expect("spawn pacquet deploy");

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!output.status.success(), "the gate must reject the deploy (stderr: {stderr})");
    assert!(
        stderr.contains("ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION"),
        "stderr must name the release-age violation; got:\n{stderr}",
    );
    assert!(
        !workspace.join("legacy-deploy/node_modules/@pnpm.e2e/foo").exists(),
        "the gate must fail before the deployed dependencies are linked",
    );
    drop((root, mock_instance));
}

#[test]
fn legacy_deploy_with_trust_lockfile_skips_source_lockfile_verification() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_policy_rejected_workspace(&workspace);

    pacquet_cmd(&workspace)
        .with_args([
            "--filter",
            "app",
            "deploy",
            "--legacy",
            "--prod",
            "--trust-lockfile",
            "legacy-deploy",
        ])
        .assert()
        .success();

    assert_eq!(
        deployed_package_version(&workspace.join("legacy-deploy"), "@pnpm.e2e/foo"),
        "100.0.0",
    );
    drop((root, mock_instance));
}
