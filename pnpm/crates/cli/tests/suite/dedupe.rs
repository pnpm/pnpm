use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
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

#[test]
fn dedupe_check_rejects_a_malformed_modules_manifest() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    pacquet.with_arg("install").assert().success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile_before = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    fs::write(workspace.join("node_modules/.modules.yaml"), "not: [valid")
        .expect("corrupt modules manifest");

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["dedupe", "--check"])
        .output()
        .expect("run dedupe check");
    assert!(!output.status.success(), "dedupe check must reject malformed .modules.yaml");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Failed to parse")
            && stderr.contains("node_modules")
            && stderr.contains(".modules.yaml")
            && stderr.contains("<input>:1:6"),
        "dedupe check must report the malformed modules manifest:\n{stderr}",
    );

    let lockfile_after = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert_eq!(lockfile_before, lockfile_after);

    drop((root, mock_instance));
}

#[test]
fn dedupe_check_keeps_valid_lockfile_pins() {
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
    pacquet.with_arg("install").assert().success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    fs::write(&manifest_path, manifest("^100.0.0")).expect("rewrite package.json");
    let lockfile_before = lockfile.replacen("specifier: 100.0.0", "specifier: ^100.0.0", 1);
    fs::write(&lockfile_path, &lockfile_before).expect("rewrite lockfile specifier");

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["dedupe", "--check"])
        .assert()
        .success();

    let lockfile_after = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert_eq!(lockfile_before, lockfile_after, "dedupe must preserve a valid existing pin");

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
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    eprintln!("STDOUT:\n{stdout}");
    assert!(stderr.is_empty(), "stderr:\n{stderr}");
    assert!(
        stdout.contains("Progress: resolved 1, reused 0, downloaded 0, added 0, done"),
        "stdout:\n{stdout}",
    );
    assert!(stdout.contains("ERR_PNPM_DEDUPE_CHECK_ISSUES"), "stdout:\n{stdout}");
    assert!(stdout.contains("Dedupe --check found changes to the lockfile"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("Importers")
            && stdout.contains("@pnpm.e2e/dep-of-pkg-with-1-dep 100.0.0 → 100.1.0"),
        "the importer's resolved version change must be rendered; stdout:\n{stdout}",
    );
    assert!(
        stdout.contains("Packages")
            && stdout.contains("+ @pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0")
            && stdout.contains("- @pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0"),
        "the added and removed snapshots must be rendered; stdout:\n{stdout}",
    );
    assert!(stdout.contains("Run pnpm dedupe to apply the changes above."), "stdout:\n{stdout}");
    assert!(
        stdout.ends_with("Run pnpm dedupe to apply the changes above.\n\n"),
        "stdout:\n{stdout}",
    );

    let ndjson_output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["dedupe", "--check", "--reporter=ndjson"])
        .output()
        .expect("run pnpm dedupe --check with the ndjson reporter");
    assert_eq!(ndjson_output.status.code(), Some(1), "ndjson check: {ndjson_output:?}");
    assert!(ndjson_output.stdout.is_empty(), "ndjson stdout: {ndjson_output:?}");
    let ndjson_stderr = String::from_utf8(ndjson_output.stderr).expect("stderr is UTF-8");
    let ndjson_records = ndjson_stderr
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid NDJSON"))
        .collect::<Vec<_>>();
    assert!(
        ndjson_records.iter().any(|record| {
            record["name"] == "pnpm:progress"
                && record["status"] == "resolved"
                && record["packageId"] == "@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0"
        }),
        "ndjson stderr:\n{ndjson_stderr}",
    );
    assert!(
        ndjson_records.iter().any(|record| {
            record["name"] == "pnpm"
                && record["level"] == "error"
                && record["err"]["code"] == "ERR_PNPM_DEDUPE_CHECK_ISSUES"
                && record["dedupeCheckIssues"]["packageIssuesByDepPath"]["added"]
                    == serde_json::json!(["@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0"])
        }),
        "ndjson stderr:\n{ndjson_stderr}",
    );

    let silent_output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["dedupe", "--check", "--reporter=silent"])
        .output()
        .expect("run pnpm dedupe --check with the silent reporter");
    assert_eq!(silent_output.status.code(), Some(1), "silent check: {silent_output:?}");
    assert!(silent_output.stdout.is_empty(), "silent stdout: {silent_output:?}");
    assert!(silent_output.stderr.is_empty(), "silent stderr: {silent_output:?}");

    let lockfile_after = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert_eq!(lockfile_before, lockfile_after, "dedupe --check must not modify pnpm-lock.yaml");

    drop((root, mock_instance));
}

/// `--check` must not write loose-mode `minimumReleaseAge` picks to
/// `minimumReleaseAgeExclude` — the check contract is "mutate nothing"
/// (see the lockfile guard in `DedupeArgs::run`).
#[test]
fn dedupe_check_does_not_persist_minimum_release_age_excludes() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    // 100 years: every version the mocked registry serves is immature, so
    // the fresh resolve behind `dedupe --check` records loose-mode picks.
    let workspace_manifest_path = workspace.join("pnpm-workspace.yaml");
    fs::write(&workspace_manifest_path, "minimumReleaseAge: 52560000\n")
        .expect("write pnpm-workspace.yaml");
    let manifest_before =
        fs::read_to_string(&workspace_manifest_path).expect("read pnpm-workspace.yaml");

    let output = pacquet.with_args(["dedupe", "--check"]).output().expect("run dedupe check");
    assert!(!output.status.success(), "no lockfile exists, so the check must report a diff");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Dedupe --check found changes to the lockfile"),
        "the failure must be the check diff itself, not an earlier error; stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    let manifest_after =
        fs::read_to_string(&workspace_manifest_path).expect("read pnpm-workspace.yaml");
    assert_eq!(
        manifest_before, manifest_after,
        "dedupe --check must not modify pnpm-workspace.yaml",
    );

    drop((root, mock_instance));
}
