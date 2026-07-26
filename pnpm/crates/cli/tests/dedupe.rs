use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, process::Command};

#[test]
fn dedupe_writes_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    pacquet.with_arg("dedupe").assert().success();

    assert!(lockfile_path.exists(), "dedupe must create pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("@pnpm.e2e/pkg-with-1-dep"),
        "lockfile must record the dependency:\n{lockfile}",
    );

    drop((root, mock_instance));
}

#[test]
fn dedupe_check_does_not_materialize_nor_write_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    // Create a lockfile first by running dedupe
    pacquet.with_arg("dedupe").assert().success();

    // Recreate a pacquet command for the --check invocation
    let pacquet_check =
        Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(&workspace);

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    assert!(lockfile_path.exists(), "dedupe must create pnpm-lock.yaml");
    let lockfile_before = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");

    pacquet_check.with_args(["dedupe", "--check"]).assert().success();

    assert!(
        !workspace.join("node_modules").exists(),
        "dedupe --check must not create node_modules",
    );
    let lockfile_after = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert_eq!(lockfile_before, lockfile_after, "dedupe --check must not modify pnpm-lock.yaml");

    drop((root, mock_instance));
}

/// pnpm's dedupe points at `pnpm peers check` when the install it runs
/// leaves peer-dependency issues behind, so the two commands agree.
#[test]
fn dedupe_warns_about_peer_dependency_issues() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/has-foo100-peer": "1.0.0",
                "@pnpm.e2e/foo": "2.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    let output = pacquet.with_arg("dedupe").output().expect("run pnpm dedupe");
    assert!(output.status.success(), "dedupe must succeed: {output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(
        stdout.contains(
            r#"Issues with peer dependencies found. Run "pnpm peers check" to list them."#
        ),
        "stdout:\n{stdout}",
    );

    let peers = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["peers", "check"])
        .output()
        .expect("run pnpm peers check");
    assert_eq!(peers.status.code(), Some(1), "peers check must confirm the issues: {peers:?}");

    drop((root, mock_instance));
}

/// A `--check` run that would rewrite the lockfile reports what it would
/// change, under pnpm's `ERR_PNPM_DEDUPE_CHECK_ISSUES`.
#[test]
fn dedupe_check_reports_the_lockfile_diff() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let manifest = |version: &str| {
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/dep-of-pkg-with-1-dep": version,
            },
        })
        .to_string()
    };
    fs::write(&manifest_path, manifest("100.0.0")).expect("write package.json");
    pacquet.with_arg("dedupe").assert().success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile_before = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    fs::write(&manifest_path, manifest("100.1.0")).expect("rewrite package.json");

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["dedupe", "--check"])
        .output()
        .expect("run pnpm dedupe --check");

    assert_eq!(output.status.code(), Some(1), "a would-change lockfile must exit 1: {output:?}");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.contains("ERR_PNPM_DEDUPE_CHECK_ISSUES"), "stderr:\n{stderr}");
    assert!(stderr.contains("Dedupe --check found changes to the lockfile"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("Importers")
            && stderr.contains("@pnpm.e2e/dep-of-pkg-with-1-dep 100.0.0 → 100.1.0"),
        "the importer's resolved version change must be rendered; stderr:\n{stderr}",
    );
    assert!(
        stderr.contains("Packages")
            && stderr.contains("+ @pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0")
            && stderr.contains("- @pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0"),
        "the added and removed snapshots must be rendered; stderr:\n{stderr}",
    );
    assert!(stderr.contains("Run `pnpm dedupe` to apply the changes above."), "stderr:\n{stderr}");

    let lockfile_after = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert_eq!(lockfile_before, lockfile_after, "dedupe --check must not modify pnpm-lock.yaml");

    drop((root, mock_instance));
}
