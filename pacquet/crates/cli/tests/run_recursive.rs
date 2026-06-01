//! Recursive-run integration tests. The build scripts run through
//! pacquet's `sh -c` executor, so the whole file is gated to Unix —
//! same as the single-package `run` tests.
#![cfg(unix)]

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{collections::HashMap, fs, path::Path};

/// Write a `pnpm-workspace.yaml` listing `names` as packages, plus a
/// `package.json` per name under its own subdirectory of `workspace`.
fn write_workspace(workspace: &Path, manifests: &[(&str, Value)]) {
    let packages = manifests.iter().map(|(name, _)| format!("  - {name}")).collect::<Vec<_>>();
    let workspace_yaml = format!("packages:\n{}\n", packages.join("\n"));
    fs::write(workspace.join("pnpm-workspace.yaml"), workspace_yaml)
        .expect("write pnpm-workspace.yaml");
    for (name, manifest) in manifests {
        let dir = workspace.join(name);
        fs::create_dir_all(&dir).expect("create project dir");
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }
}

/// Map each summary entry to `(basename, status)` so assertions don't
/// depend on the absolute tempdir path used as the key.
fn summary_statuses(workspace: &Path) -> HashMap<String, String> {
    let contents =
        fs::read_to_string(workspace.join("pnpm-exec-summary.json")).expect("read summary file");
    let value: Value = serde_json::from_str(&contents).expect("parse summary file");
    value["executionStatus"]
        .as_object()
        .expect("executionStatus is an object")
        .iter()
        .map(|(prefix, entry)| {
            let basename = Path::new(prefix)
                .file_name()
                .expect("prefix has a basename")
                .to_string_lossy()
                .into_owned();
            let status = entry["status"].as_str().expect("status is a string").to_string();
            (basename, status)
        })
        .collect()
}

fn build_writes_marker(workspace: &Path, name: &str) -> Value {
    let marker = workspace.join(format!("ran-{name}.txt"));
    json!({
        "name": name,
        "version": "1.0.0",
        "scripts": { "build": format!(r#"touch "{}""#, marker.display()) },
    })
}

/// `pacquet -r run <script>` runs the script in every workspace project,
/// in topological order. Mirrors the ordering pnpm's recursive run
/// produces from the workspace dependency graph.
#[test]
fn recursive_run_executes_script_in_every_project() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("project-1", build_writes_marker(&workspace, "project-1")),
            ("project-2", build_writes_marker(&workspace, "project-2")),
            ("project-3", build_writes_marker(&workspace, "project-3")),
        ],
    );

    pacquet.with_arg("-r").with_arg("run").with_arg("build").assert().success();

    for name in ["project-1", "project-2", "project-3"] {
        assert!(
            workspace.join(format!("ran-{name}.txt")).exists(),
            "{name} build script should have run",
        );
    }

    drop(root);
}

/// `pacquet -r run --resume-from <pkg>` skips every chunk that sorts
/// before the chunk containing `<pkg>`. With `project-2` and `project-3`
/// both depending on `project-1`, the sorted chunks are
/// `[[project-1], [project-2, project-3]]`; resuming from `project-3`
/// drops the first chunk, so only `project-2` and `project-3` run.
///
/// Ports pnpm's
/// [`runRecursive.ts:817`](https://github.com/pnpm/pnpm/blob/8eb1be4988/exec/commands/test/runRecursive.ts#L817)
/// `` `pnpm -r --resume-from run` should executed from given package ``.
#[test]
fn recursive_run_resume_from_starts_at_the_given_package() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let dependent = |name: &str| {
        let mut manifest = build_writes_marker(&workspace, name);
        manifest["dependencies"] = json!({ "project-1": "1" });
        manifest
    };
    write_workspace(
        &workspace,
        &[
            ("project-1", build_writes_marker(&workspace, "project-1")),
            ("project-2", dependent("project-2")),
            ("project-3", dependent("project-3")),
        ],
    );

    pacquet
        .with_arg("-r")
        .with_arg("run")
        .with_arg("--resume-from")
        .with_arg("project-3")
        .with_arg("build")
        .assert()
        .success();

    assert!(
        !workspace.join("ran-project-1.txt").exists(),
        "project-1 sorts before the resume point and must be skipped",
    );
    assert!(workspace.join("ran-project-2.txt").exists(), "project-2 should run");
    assert!(workspace.join("ran-project-3.txt").exists(), "project-3 should run");

    drop(root);
}

/// An unknown `--resume-from` package fails with pnpm's
/// `ERR_PNPM_RESUME_FROM_NOT_FOUND`. Ports the error path of pnpm's
/// `getResumedPackageChunks`.
#[test]
fn recursive_run_resume_from_unknown_package_errors() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &[("project-1", build_writes_marker(&workspace, "project-1"))]);

    let output = pacquet
        .with_arg("-r")
        .with_arg("run")
        .with_arg("--resume-from")
        .with_arg("does-not-exist")
        .with_arg("build")
        .output()
        .expect("spawn pacquet");
    assert!(!output.status.success(), "an unknown resume-from package must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_RESUME_FROM_NOT_FOUND"),
        "stderr should carry the resume-from error code, got: {stderr}",
    );

    drop(root);
}

/// `pacquet -r run --report-summary` writes `pnpm-exec-summary.json`
/// recording every package's status: `passed`, `failure`, or `skipped`
/// (no matching script). With `--no-bail` every package runs even after
/// a failure, and the overall run fails with `ERR_PNPM_RECURSIVE_FAIL`.
///
/// Ports pnpm's
/// [`runRecursive.ts:956`](https://github.com/pnpm/pnpm/blob/8eb1be4988/exec/commands/test/runRecursive.ts#L956)
/// `pnpm recursive run report summary` (whose `DEFAULT_OPTS` set
/// `bail: false`).
#[test]
fn recursive_run_report_summary_records_every_package_status() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let build = |name: &str, body: &str| json!({ "name": name, "version": "1.0.0", "scripts": { "build": body } });
    write_workspace(
        &workspace,
        &[
            ("project-1", build("project-1", "true")),
            ("project-2", build("project-2", "exit 1")),
            ("project-3", build("project-3", "true")),
            ("project-4", build("project-4", "exit 1")),
            ("project-5", json!({ "name": "project-5", "version": "1.0.0" })),
        ],
    );

    let output = pacquet
        .with_arg("-r")
        .with_arg("run")
        .with_arg("--report-summary")
        .with_arg("--no-bail")
        .with_arg("build")
        .output()
        .expect("spawn pacquet");
    assert!(!output.status.success(), "a run with failing packages must fail overall");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_RECURSIVE_FAIL"),
        "stderr should carry the recursive-fail code, got: {stderr}",
    );

    let statuses = summary_statuses(&workspace);
    let expected = [
        ("project-1", "passed"),
        ("project-2", "failure"),
        ("project-3", "passed"),
        ("project-4", "failure"),
        ("project-5", "skipped"),
    ];
    for (name, status) in expected {
        assert_eq!(statuses.get(name).map(String::as_str), Some(status), "status of {name}");
    }

    drop(root);
}

/// With bail on (the default) and `--report-summary`, the first failing
/// script aborts the run *after* the summary is written: the run fails
/// with `ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL`, the summary records the
/// failed package, and a package that sorts after it stays `queued`
/// because it never ran. Covers the bail + report-summary branch.
#[test]
fn recursive_run_bail_writes_summary_then_stops_at_first_failure() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let build = |name: &str, body: &str| json!({ "name": name, "version": "1.0.0", "scripts": { "build": body } });
    write_workspace(
        &workspace,
        &[("project-1", build("project-1", "exit 1")), ("project-2", build("project-2", "true"))],
    );

    let output = pacquet
        .with_arg("-r")
        .with_arg("run")
        .with_arg("--report-summary")
        .with_arg("build")
        .output()
        .expect("spawn pacquet");
    assert!(!output.status.success(), "a failing script with bail on must fail the run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL"),
        "stderr should carry the bail first-fail code, got: {stderr}",
    );

    let statuses = summary_statuses(&workspace);
    assert_eq!(statuses.get("project-1").map(String::as_str), Some("failure"), "project-1 failed");
    assert_eq!(
        statuses.get("project-2").map(String::as_str),
        Some("queued"),
        "project-2 never ran because bail stopped at project-1",
    );

    drop(root);
}

/// With bail on (the default) and `--report-summary` *off*, a failing
/// script still aborts with `ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL`, but no
/// summary file is written. Covers the report-summary-off side of the
/// bail block.
#[test]
fn recursive_run_bail_without_report_summary_writes_no_file() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let build = |name: &str, body: &str| json!({ "name": name, "version": "1.0.0", "scripts": { "build": body } });
    write_workspace(
        &workspace,
        &[("project-1", build("project-1", "exit 1")), ("project-2", build("project-2", "true"))],
    );

    let output =
        pacquet.with_arg("-r").with_arg("run").with_arg("build").output().expect("spawn pacquet");
    assert!(!output.status.success(), "a failing script with bail on must fail the run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL"),
        "stderr should carry the bail first-fail code, got: {stderr}",
    );
    assert!(
        !workspace.join("pnpm-exec-summary.json").exists(),
        "no summary file should be written without --report-summary",
    );

    drop(root);
}

/// A recursive run for a script no package defines fails with pnpm's
/// `ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT`. Covers the no-script branch.
#[test]
fn recursive_run_errors_when_no_package_has_the_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("project-1", build_writes_marker(&workspace, "project-1")),
            ("project-2", build_writes_marker(&workspace, "project-2")),
        ],
    );

    let output =
        pacquet.with_arg("-r").with_arg("run").with_arg("lint").output().expect("spawn pacquet");
    assert!(!output.status.success(), "a script no package defines must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT"),
        "stderr should carry the no-script code, got: {stderr}",
    );

    drop(root);
}

/// `--if-present` turns the no-script case into a clean no-op: the run
/// exits 0 even though no package defines the script. Guards the
/// `!args.if_present` side of the no-script branch.
#[test]
fn recursive_run_if_present_is_a_noop_when_no_package_has_the_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &[("project-1", build_writes_marker(&workspace, "project-1"))]);

    pacquet
        .with_arg("-r")
        .with_arg("run")
        .with_arg("--if-present")
        .with_arg("lint")
        .assert()
        .success();

    drop(root);
}
