use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    command_env::CommandTestExt,
};
use std::{fs, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
}

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
fn dedupe_materializes_node_modules_unless_lockfile_only() {
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

    pacquet.with_args(["dedupe", "--lockfile-only"]).assert().success();

    assert!(
        workspace.join("pnpm-lock.yaml").exists(),
        "dedupe --lockfile-only must write pnpm-lock.yaml",
    );
    assert!(
        !workspace.join("node_modules").exists(),
        "dedupe --lockfile-only must not create node_modules",
    );

    let pacquet =
        Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(&workspace);
    pacquet.with_arg("dedupe").assert().success();

    assert!(
        workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep").exists(),
        "dedupe must link the dependency into node_modules",
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

    pacquet.with_args(["dedupe", "--lockfile-only"]).assert().success();

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

/// `strictPeerDependencies: true` turns the same peer-dependency issues
/// [`dedupe_warns_about_peer_dependency_issues`] only warns about into a
/// hard failure, matching the TypeScript CLI's `ERR_PNPM_PEER_DEP_ISSUES`.
#[test]
fn dedupe_fails_on_peer_dependency_issues_when_strict() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(workspace.join("pnpm-workspace.yaml"), "strictPeerDependencies: true\n")
        .expect("write pnpm-workspace.yaml");
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
    assert!(
        !output.status.success(),
        "dedupe must fail when strictPeerDependencies is true: {output:?}",
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("ERR_PNPM_PEER_DEP_ISSUES"), "stdout:\n{stdout}");
    assert!(stdout.contains("Unmet peer dependencies"), "stdout:\n{stdout}");
    assert!(stdout.contains("@pnpm.e2e/foo"), "stdout:\n{stdout}");
    assert!(stdout.contains("Wanted:"), "stdout:\n{stdout}");
    assert!(stdout.contains("strictPeerDependencies: false"), "stdout:\n{stdout}");
    assert!(!stdout.contains("autoInstallPeers: true"), "stdout:\n{stdout}");

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    assert!(lockfile_path.exists(), "dedupe still writes the lockfile before failing");

    drop((root, mock_instance));
}

/// A peer nothing installed at all also earns the `autoInstallPeers` hint,
/// which the bad-peer failure above leaves out.
#[test]
fn dedupe_strict_failure_hints_at_auto_install_peers_for_a_missing_peer() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "strictPeerDependencies: true\nautoInstallPeers: false\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/has-foo100-peer": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    let output = pacquet.with_arg("dedupe").output().expect("run pnpm dedupe");
    assert!(
        !output.status.success(),
        "dedupe must fail when strictPeerDependencies is true: {output:?}",
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("missing peer"), "stdout:\n{stdout}");
    assert!(stdout.contains("autoInstallPeers: true"), "stdout:\n{stdout}");
    assert!(stdout.contains("strictPeerDependencies: false"), "stdout:\n{stdout}");

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
    // An explicit cutoff turns strict mode on by default, which would abort
    // the resolve before the check can report its diff.
    let workspace_manifest_path = workspace.join("pnpm-workspace.yaml");
    fs::write(
        &workspace_manifest_path,
        "minimumReleaseAge: 52560000\nminimumReleaseAgeStrict: false\n",
    )
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

/// A lockfile whose snapshot keys carry no `(peer)` segment for an
/// optional peer that only a sibling's subtree provides — the shape
/// pnpm 11 writes for this graph — must be re-keyed completely.
/// The graph mirrors pnpm/pnpm#14455: `optional-peer-c-consumer` and its
/// auto-installed peer both reach `optional-peer-c-host`, whose optional
/// `peer-c` is provided by `abc-regular-deps`. One `dedupe` pass must
/// re-key every affected snapshot — the consumer's own key included —
/// and a second pass must change nothing.
#[test]
fn dedupe_re_keys_a_hoisted_optional_peer_in_one_pass() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/abc-regular-deps": "1.0.0",
                "@pnpm.e2e/optional-peer-c-consumer": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    pacquet_at(&workspace).with_args(["install", "--lockfile-only"]).assert().success();
    let converged = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    let consumer_key = concat!(
        "@pnpm.e2e/optional-peer-c-consumer@1.0.0",
        "(@pnpm.e2e/optional-peer-c-consumer-peer@1.0.0(@pnpm.e2e/peer-c@1.0.0))",
        "(@pnpm.e2e/peer-c@1.0.0)",
    );
    assert!(
        converged.contains(consumer_key),
        "a fresh resolution must hoist peer-c into the consumer's key:\n{converged}",
    );

    let hoisted_host_snapshot = concat!(
        "  '@pnpm.e2e/optional-peer-c-host@1.0.0(@pnpm.e2e/peer-c@1.0.0)':\n",
        "    optionalDependencies:\n",
        "      '@pnpm.e2e/peer-c': 1.0.0\n",
    );
    assert!(converged.contains(hoisted_host_snapshot), "unexpected lockfile shape:\n{converged}");
    let stale = converged
        .replace(hoisted_host_snapshot, "  '@pnpm.e2e/optional-peer-c-host@1.0.0': {}\n")
        .replace("(@pnpm.e2e/peer-c@1.0.0)", "");
    fs::write(&lockfile_path, &stale).expect("write the pre-hoisting lockfile");

    pacquet_at(&workspace).with_args(["dedupe", "--lockfile-only"]).assert().success();
    let first_pass = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    eprintln!("first pass:\n{first_pass}\nfresh resolution:\n{converged}");
    assert_eq!(first_pass, converged, "one dedupe pass must reach the fresh resolution");

    pacquet_at(&workspace).with_args(["dedupe", "--lockfile-only"]).assert().success();
    let second_pass = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    eprintln!("second pass:\n{second_pass}");
    assert_eq!(second_pass, first_pass, "a second dedupe pass must change nothing");

    drop((root, mock_instance));
}
