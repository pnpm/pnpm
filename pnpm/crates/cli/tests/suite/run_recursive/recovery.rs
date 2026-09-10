use super::{
    Command, CommandExtra, CommandTempCwd, Value, assert_recursive_run_bail_cancels_in_flight,
    build_appends_run_order, build_writes_marker, fs, json, summary_statuses, write_workspace,
};
use assert_cmd::{assert::OutputAssertExt, cargo::CommandCargoExt};

/// `--no-sort` disregards ordering entirely, so there is no graph for
/// `--reverse` to turn around or for `--resume-from` to skip the anchor's
/// dependencies in — both are no-ops and every project runs in workspace
/// order, exactly as in pnpm.
#[test]
fn recursive_run_no_sort_makes_reverse_and_resume_no_ops() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("z-first", build_appends_run_order("z-first")),
            ("m-middle", build_appends_run_order("m-middle")),
            ("a-last", build_appends_run_order("a-last")),
        ],
    );

    pacquet
        .with_args([
            "--workspace-concurrency=1",
            "--no-sort",
            "--reverse",
            "--resume-from=m-middle",
            "-r",
            "run",
            "build",
        ])
        .assert()
        .success();

    let order = fs::read_to_string(workspace.join("order.log")).expect("read order log");
    assert_eq!(order, "a-last\nm-middle\nz-first\n");

    drop(root);
}

/// `pacquet -r run --resume-from <pkg>` skips every chunk that sorts
/// before the chunk containing `<pkg>`. With `project-2` and `project-3`
/// both depending on `project-1`, the sorted chunks are
/// `[[project-1], [project-2, project-3]]`; resuming from `project-3`
/// drops the first chunk, so only `project-2` and `project-3` run.
#[test]
fn recursive_run_resume_from_starts_at_the_given_package() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let dependent = |name: &str| {
        let mut manifest = build_writes_marker(name);
        manifest["dependencies"] = json!({ "project-1": "workspace:*" });
        manifest
    };
    write_workspace(
        &workspace,
        &[
            ("project-1", build_writes_marker("project-1")),
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
        !workspace.join("project-1").join("ran.txt").exists(),
        "project-1 sorts before the resume point and must be skipped",
    );
    assert!(workspace.join("project-2").join("ran.txt").exists(), "project-2 should run");
    assert!(workspace.join("project-3").join("ran.txt").exists(), "project-3 should run");

    drop(root);
}

#[test]
fn recursive_run_resumes_from_exactly_the_tasks_that_passed_before_a_failure() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            (
                "dependency",
                json!({
                    "name": "dependency",
                    "version": "1.0.0",
                    "scripts": { "build": "echo dependency >> ../order.log; [ ! -e ../fail ]" },
                }),
            ),
            (
                "anchor",
                json!({
                    "name": "anchor",
                    "version": "1.0.0",
                    "dependencies": { "dependency": "workspace:*" },
                    "scripts": { "build": "echo anchor >> ../order.log" },
                }),
            ),
            (
                "completed",
                json!({
                    "name": "completed",
                    "version": "1.0.0",
                    "scripts": { "build": "echo completed >> ../order.log" },
                }),
            ),
        ],
    );
    fs::write(workspace.join("fail"), "").expect("write failure marker");

    pacquet
        .with_args(["--no-bail", "--workspace-concurrency=1", "-r", "run", "build"])
        .assert()
        .failure();
    let first_run = fs::read_to_string(workspace.join("order.log")).expect("read first run");
    let mut first_tasks: Vec<&str> = first_run.lines().collect();
    first_tasks.sort_unstable();
    assert_eq!(first_tasks, ["completed", "dependency"]);

    fs::remove_file(workspace.join("fail")).expect("remove failure marker");
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["--workspace-concurrency=1", "--resume-from=anchor", "-r", "run", "build"])
        .assert()
        .success();

    let order = fs::read_to_string(workspace.join("order.log")).expect("read resumed run");
    assert!(order.ends_with("dependency\nanchor\n"), "unfinished dependency must rerun: {order}");
    assert_eq!(order.lines().filter(|task| *task == "completed").count(), 1);
    let state_dir = workspace.join("node_modules").join(".pnpm-task-run-state-v1");
    let latest: Value = serde_json::from_str(
        &fs::read_to_string(state_dir.join("latest.json")).expect("read latest state pointer"),
    )
    .expect("parse latest state pointer");
    let latest_journal = state_dir.join(format!(
        "{}.{}.jsonl",
        latest["invocation"].as_str().expect("latest invocation"),
        latest["run"].as_str().expect("latest run"),
    ));
    assert!(!latest_journal.exists(), "successful resume removes its current checkpoint");

    drop(root);
}

#[test]
fn recursive_run_does_not_persist_a_task_skipped_by_the_recursion_guard() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            (
                "origin",
                json!({
                    "name": "origin",
                    "version": "1.0.0",
                    "scripts": {
                        "build": r#"node -e "require('fs').appendFileSync('../order.log', 'origin\n')""#,
                    },
                }),
            ),
            (
                "anchor",
                json!({
                    "name": "anchor",
                    "version": "1.0.0",
                    "dependencies": { "origin": "workspace:*" },
                    "scripts": {
                        "build": r#"node -e "require('fs').appendFileSync('../order.log', 'anchor\n')""#,
                    },
                }),
            ),
            (
                "failure",
                json!({
                    "name": "failure",
                    "version": "1.0.0",
                    "scripts": {
                        "build": r#"node -e "const fs=require('fs');fs.appendFileSync('../order.log','failure\n');if(fs.existsSync('../fail'))process.exit(1)""#,
                    },
                }),
            ),
        ],
    );
    fs::write(workspace.join("fail"), "").expect("write failure marker");
    let origin = fs::canonicalize(workspace.join("origin")).expect("canonicalize origin");

    pacquet
        .with_env("npm_lifecycle_event", "build")
        .with_env("PNPM_SCRIPT_SRC_DIR", origin.to_string_lossy().as_ref())
        .with_args(["--no-bail", "--workspace-concurrency=1", "-r", "run", "build"])
        .assert()
        .failure();
    let first_run = fs::read_to_string(workspace.join("order.log")).expect("read first run");
    assert!(!first_run.lines().any(|task| task == "origin"), "origin must be recursion-guarded");

    fs::remove_file(workspace.join("fail")).expect("remove failure marker");
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["--workspace-concurrency=1", "--resume-from=anchor", "-r", "run", "build"])
        .assert()
        .success();

    let order = fs::read_to_string(workspace.join("order.log")).expect("read resumed run");
    assert_eq!(order.lines().filter(|task| *task == "origin").count(), 1, "{order}");

    drop(root);
}

/// An unknown `--resume-from` package fails with
/// `ERR_PNPM_RESUME_FROM_NOT_FOUND`.
#[test]
fn recursive_run_resume_from_unknown_package_errors() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &[("project-1", build_writes_marker("project-1"))]);

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

#[test]
fn recursive_run_bail_cancels_in_flight_processes() {
    assert_recursive_run_bail_cancels_in_flight(false);
}

#[test]
fn recursive_run_bail_cancels_in_flight_shell_emulator_tasks() {
    assert_recursive_run_bail_cancels_in_flight(true);
}

#[test]
fn recursive_run_reads_bail_from_workspace_config() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            (
                "fails",
                json!({
                    "name": "fails",
                    "version": "1.0.0",
                    "scripts": { "build": "exit 1" },
                }),
            ),
            (
                "later-continues",
                json!({
                    "name": "later-continues",
                    "version": "1.0.0",
                    "scripts": { "build": "touch ran.txt" },
                }),
            ),
        ],
    );
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - fails\n  - later-continues\nbail: false\n",
    )
    .expect("write workspace settings");

    // Concurrency 1 makes the failing project run first, so the later
    // project only runs because `bail: false` was read from the file.
    let output = pacquet
        .with_args(["--workspace-concurrency=1", "-r", "run", "build"])
        .output()
        .expect("run recursive script");

    assert!(!output.status.success(), "the failed project must still fail the command");
    assert!(
        workspace.join("later-continues/ran.txt").exists(),
        "bail: false must keep running unrelated projects after a failure",
    );

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
        .with_arg("--workspace-concurrency=1")
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

/// Recursion guard: when `npm_lifecycle_event` matches the requested
/// script AND `PNPM_SCRIPT_SRC_DIR` matches a project root, that
/// project is skipped so a script that itself invokes `pacquet -r run
/// <name>` doesn't recurse without bound.
#[test]
fn recursive_run_recursion_guard_skips_originating_project() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("project-1", build_writes_marker("project-1")),
            ("project-2", build_writes_marker("project-2")),
        ],
    );

    // Pretend we're already inside `project-1`'s `build` lifecycle —
    // pnpm's recursion guard should leave `project-1` alone while
    // still running `project-2`. Canonicalize the path so the env-var
    // value matches what `find_workspace_projects` derives internally:
    // on macOS the tempdir lives under `/var/folders/...` (a symlink to
    // `/private/var/folders/...`) and the CLI canonicalizes its `--dir`,
    // so the project roots pacquet compares against are the
    // `/private/...` form.
    let project_1 = fs::canonicalize(workspace.join("project-1")).expect("canonicalize project-1");
    pacquet
        .with_env("npm_lifecycle_event", "build")
        .with_env("PNPM_SCRIPT_SRC_DIR", project_1.to_string_lossy().as_ref())
        .with_arg("-r")
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();

    assert!(
        !workspace.join("project-1").join("ran.txt").exists(),
        "the originating project must be recursion-guarded and skipped",
    );
    assert!(
        workspace.join("project-2").join("ran.txt").exists(),
        "other projects should still run",
    );

    drop(root);
}
