use super::{
    CommandExtra, CommandTempCwd, Value, build_appends_run_order, build_writes_marker, fs, json,
    summary_statuses, write_workspace,
};
use assert_cmd::assert::OutputAssertExt;

#[test]
fn filter_keeps_dependency_tasks_outside_the_selection_from_running() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            (
                "app",
                json!({
                    "name": "app",
                    "version": "1.0.0",
                    "dependencies": { "lib": "workspace:*" },
                    "scripts": { "build": "echo app >> ../order.log" },
                }),
            ),
            ("lib", build_appends_run_order("lib")),
        ],
    );
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - app\n  - lib\ntasks:\n  build:\n    dependsOn: ['^build']\n",
    )
    .expect("write workspace settings");

    pacquet.with_args(["--filter", "app", "run", "build"]).assert().success();

    let order = fs::read_to_string(workspace.join("order.log")).expect("read order log");
    assert_eq!(order, "app\n");

    drop(root);
}

/// `slow` waits for a marker only `mid` writes, and `mid` may start only
/// once `dep` is done — so the run completes only if `mid` is dispatched
/// while the unrelated `slow` is still in flight. Any barrier between
/// dependency-independent tasks deadlocks this fixture.
#[test]
fn task_starts_as_soon_as_its_dependencies_finish() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("dep", build_appends_run_order("dep")),
            (
                "mid",
                json!({
                    "name": "mid",
                    "version": "1.0.0",
                    "dependencies": { "dep": "workspace:*" },
                    "scripts": { "build": "echo mid >> ../order.log && touch ../slow-marker" },
                }),
            ),
            (
                "slow",
                json!({
                    "name": "slow",
                    "version": "1.0.0",
                    "scripts": {
                        "build": concat!(
                            r#"node -e "const fs = require('fs'); const started = Date.now(); (function poll () { if (fs.existsSync('../slow-marker')) process.exit(0); if (Date.now() - started > 30000) process.exit(1); setTimeout(poll, 50) })()""#,
                            " && echo slow >> ../order.log",
                        ),
                    },
                }),
            ),
        ],
    );

    pacquet.with_args(["--workspace-concurrency=2", "-r", "run", "build"]).assert().success();

    let order = fs::read_to_string(workspace.join("order.log")).expect("read order log");
    assert_eq!(order, "dep\nmid\nslow\n");

    drop(root);
}

#[test]
fn depends_on_runs_the_tasks_a_task_depends_on_in_dependency_order() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let scripts = |name: &str| {
        json!({
            "name": name,
            "version": "1.0.0",
            "dependencies": if name == "project-a" { json!({ "project-b": "workspace:*" }) } else { json!({}) },
            "scripts": {
                "build": format!("echo {name}-build >> ../order.log"),
                "test": format!("echo {name}-test >> ../order.log"),
            },
        })
    };
    write_workspace(
        &workspace,
        &[("project-a", scripts("project-a")), ("project-b", scripts("project-b"))],
    );
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        concat!(
            "packages:\n  - project-a\n  - project-b\n",
            "tasks:\n",
            "  build:\n    dependsOn: ['^build']\n",
            "  test:\n    dependsOn: ['build']\n",
        ),
    )
    .expect("write workspace settings");

    pacquet.with_args(["-r", "run", "--report-summary", "test"]).assert().success();

    let order = fs::read_to_string(workspace.join("order.log")).expect("read order log");
    let lines: Vec<&str> = order.lines().collect();
    dbg!(&lines);
    let position = |line: &str| lines.iter().position(|found| *found == line).expect(line);
    assert!(position("project-b-build") < position("project-a-build"));
    assert!(position("project-a-build") < position("project-a-test"));
    assert!(position("project-b-build") < position("project-b-test"));

    // The tasks `dependsOn` pulled in get `#`-qualified summary keys; the
    // requested tasks keep the bare project directory.
    let statuses = summary_statuses(&workspace);
    assert_eq!(statuses.get("project-a").map(String::as_str), Some("passed"));
    assert_eq!(statuses.get("project-a#build").map(String::as_str), Some("passed"));

    drop(root);
}

/// `dependency`'s lint waits for the marker `dependent`'s lint writes:
/// only possible when the explicitly empty `dependsOn` frees the lint
/// tasks from the project-graph order.
#[test]
fn explicitly_empty_depends_on_starts_without_waiting() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            (
                "dependency",
                json!({
                    "name": "dependency",
                    "version": "1.0.0",
                    "scripts": {
                        "lint": concat!(
                            r#"node -e "const fs = require('fs'); const started = Date.now(); (function poll () { if (fs.existsSync('../lint-marker')) process.exit(0); if (Date.now() - started > 30000) process.exit(1); setTimeout(poll, 50) })()""#,
                            " && echo dependency >> ../order.log",
                        ),
                    },
                }),
            ),
            (
                "dependent",
                json!({
                    "name": "dependent",
                    "version": "1.0.0",
                    "dependencies": { "dependency": "workspace:*" },
                    "scripts": { "lint": "echo dependent >> ../order.log && touch ../lint-marker" },
                }),
            ),
        ],
    );
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - dependency\n  - dependent\ntasks:\n  lint: {}\n",
    )
    .expect("write workspace settings");

    pacquet.with_args(["--workspace-concurrency=2", "-r", "run", "lint"]).assert().success();

    let order = fs::read_to_string(workspace.join("order.log")).expect("read order log");
    assert_eq!(order, "dependent\ndependency\n");

    drop(root);
}

#[test]
fn missing_script_is_reported_skipped_and_does_not_sever_the_chain() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            (
                "project-a",
                json!({
                    "name": "project-a",
                    "version": "1.0.0",
                    "dependencies": { "project-b": "workspace:*" },
                    "scripts": { "build": "echo project-a >> ../order.log" },
                }),
            ),
            (
                "project-b",
                json!({
                    "name": "project-b",
                    "version": "1.0.0",
                    "dependencies": { "project-c": "workspace:*" },
                }),
            ),
            (
                "project-c",
                json!({
                    "name": "project-c",
                    "version": "1.0.0",
                    "scripts": { "build": "echo project-c >> ../order.log" },
                }),
            ),
        ],
    );

    pacquet.with_args(["-r", "run", "--report-summary", "build"]).assert().success();

    let order = fs::read_to_string(workspace.join("order.log")).expect("read order log");
    assert_eq!(order, "project-c\nproject-a\n");
    let statuses = summary_statuses(&workspace);
    assert_eq!(statuses.get("project-a").map(String::as_str), Some("passed"));
    assert_eq!(statuses.get("project-b").map(String::as_str), Some("skipped"));
    assert_eq!(statuses.get("project-c").map(String::as_str), Some("passed"));

    drop(root);
}

/// The failure that blocked the dependent is already counted; the skipped
/// dependent must not turn one failure into two.
#[test]
fn no_bail_skips_dependents_of_a_failed_task_and_runs_unrelated_ones() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            (
                "project-a",
                json!({
                    "name": "project-a",
                    "version": "1.0.0",
                    "dependencies": { "project-b": "workspace:*" },
                    "scripts": { "build": "echo project-a >> ../order.log" },
                }),
            ),
            (
                "project-b",
                json!({
                    "name": "project-b",
                    "version": "1.0.0",
                    "scripts": { "build": "exit 1" },
                }),
            ),
            (
                "project-c",
                json!({
                    "name": "project-c",
                    "version": "1.0.0",
                    "scripts": { "build": "echo project-c >> ../order.log" },
                }),
            ),
        ],
    );

    let output = pacquet
        .with_args(["--no-bail", "-r", "run", "--report-summary", "build"])
        .output()
        .expect("run recursive script");
    assert!(!output.status.success(), "the failed project must fail the run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("failed in 1 packages"), "one failure, not two: {stderr}");

    let order = fs::read_to_string(workspace.join("order.log")).expect("read order log");
    assert_eq!(order, "project-c\n");
    let statuses = summary_statuses(&workspace);
    assert_eq!(statuses.get("project-a").map(String::as_str), Some("skipped"));
    assert_eq!(statuses.get("project-b").map(String::as_str), Some("failure"));
    assert_eq!(statuses.get("project-c").map(String::as_str), Some("passed"));

    drop(root);
}

#[test]
fn workspace_dependency_cycle_is_an_error_naming_the_participating_tasks() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let cyclic = |name: &str, dependency: &str| {
        json!({
            "name": name,
            "version": "1.0.0",
            "dependencies": { dependency: "workspace:*" },
            "scripts": { "build": format!("echo {name} >> ../order.log") },
        })
    };
    write_workspace(
        &workspace,
        &[
            ("project-a", cyclic("project-a", "project-b")),
            ("project-b", cyclic("project-b", "project-a")),
        ],
    );

    let output = pacquet.with_args(["-r", "run", "build"]).output().expect("run recursive script");
    assert!(!output.status.success(), "a task cycle must fail the run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_TASK_CYCLE"), "stderr: {stderr}");
    assert!(stderr.contains("project-a#build"), "stderr: {stderr}");
    assert!(stderr.contains("project-b#build"), "stderr: {stderr}");
    assert!(!workspace.join("order.log").exists(), "nothing may run");

    drop(root);
}

#[test]
fn depends_on_cycle_is_an_error() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[(
            "project-a",
            json!({
                "name": "project-a",
                "version": "1.0.0",
                "scripts": { "build": "echo build", "test": "echo test" },
            }),
        )],
    );
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        concat!(
            "packages:\n  - project-a\n",
            "tasks:\n",
            "  build:\n    dependsOn: ['test']\n",
            "  test:\n    dependsOn: ['build']\n",
        ),
    )
    .expect("write workspace settings");

    let output = pacquet.with_args(["-r", "run", "test"]).output().expect("run recursive script");
    assert!(!output.status.success(), "a task cycle must fail the run");
    assert!(String::from_utf8_lossy(&output.stderr).contains("ERR_PNPM_TASK_CYCLE"));

    drop(root);
}

#[test]
fn dry_run_prints_one_stable_linearization_and_runs_nothing() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            (
                "project-a",
                json!({
                    "name": "project-a",
                    "version": "1.0.0",
                    "dependencies": { "project-b": "workspace:*" },
                    "scripts": { "build": "echo project-a >> ../order.log" },
                }),
            ),
            (
                "project-b",
                json!({
                    "name": "project-b",
                    "version": "1.0.0",
                    "dependencies": { "project-c": "workspace:*" },
                }),
            ),
            (
                "project-c",
                json!({
                    "name": "project-c",
                    "version": "1.0.0",
                    "scripts": { "build": "echo project-c >> ../order.log" },
                }),
            ),
        ],
    );

    let output = pacquet.with_args(["-r", "run", "--dry-run", "build"]).output().expect("dry run");
    assert!(output.status.success(), "dry run failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(
            "project-c#build\nproject-b#build (skipped: no such script)\nproject-a#build\n",
        ),
        "stdout: {stdout}",
    );
    assert!(!workspace.join("order.log").exists(), "a dry run must run nothing");

    drop(root);
}

#[test]
fn dry_run_json_emits_the_tasks_and_their_resolved_edges() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            (
                "project-a",
                json!({
                    "name": "project-a",
                    "version": "1.0.0",
                    "dependencies": { "project-b": "workspace:*" },
                    "scripts": { "build": "echo a", "test": "echo a" },
                }),
            ),
            (
                "project-b",
                json!({
                    "name": "project-b",
                    "version": "1.0.0",
                    "scripts": { "build": "echo b" },
                }),
            ),
        ],
    );
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        concat!(
            "packages:\n  - project-a\n  - project-b\n",
            "tasks:\n",
            "  build:\n    dependsOn: ['^build']\n",
            "  test:\n    dependsOn: ['build']\n",
        ),
    )
    .expect("write workspace settings");

    let output =
        pacquet.with_args(["-r", "run", "--dry-run", "--json", "test"]).output().expect("dry run");
    assert!(output.status.success(), "dry run failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout.find('{').expect("stdout carries a JSON document");
    let document: Value = serde_json::from_str(&stdout[json_start..]).expect("parse dry-run JSON");
    assert_eq!(
        document,
        json!({
            "tasks": [
                {
                    "project": "project-a",
                    "script": "build",
                    "missingScript": false,
                    "dependsOn": [{ "project": "project-b", "script": "build" }],
                },
                {
                    "project": "project-a",
                    "script": "test",
                    "missingScript": false,
                    "dependsOn": [{ "project": "project-a", "script": "build" }],
                },
                {
                    "project": "project-b",
                    "script": "build",
                    "missingScript": false,
                    "dependsOn": [],
                },
                {
                    "project": "project-b",
                    "script": "test",
                    "missingScript": true,
                    "dependsOn": [{ "project": "project-b", "script": "build" }],
                },
            ],
        }),
    );

    drop(root);
}

#[test]
fn dry_run_outside_a_recursive_run_is_an_error() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &[("project-a", build_writes_marker("project-a"))]);

    let output = pacquet
        .with_current_dir(workspace.join("project-a"))
        .with_args(["run", "--dry-run", "build"])
        .output()
        .expect("run dry-run");
    assert!(!output.status.success(), "--dry-run without -r must error");
    assert!(String::from_utf8_lossy(&output.stderr).contains("ERR_PNPM_DRY_RUN_NOT_RECURSIVE"));

    drop(root);
}

/// Every requested task was skipped because its build dependency failed;
/// the run must report that failure, not `RECURSIVE_RUN_NO_SCRIPT`.
#[test]
fn failed_upstream_task_is_reported_as_the_failure_not_a_missing_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[(
            "project-a",
            json!({
                "name": "project-a",
                "version": "1.0.0",
                "scripts": { "build": "exit 1", "test": "echo test" },
            }),
        )],
    );
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - project-a\ntasks:\n  test:\n    dependsOn: ['build']\n",
    )
    .expect("write workspace settings");

    let output = pacquet
        .with_args(["--no-bail", "-r", "run", "--report-summary", "test"])
        .output()
        .expect("run recursive script");
    assert!(!output.status.success(), "the failed build must fail the run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("failed in 1 packages"), "stderr: {stderr}");
    assert!(!stderr.contains("RECURSIVE_RUN_NO_SCRIPT"), "stderr: {stderr}");

    let statuses = summary_statuses(&workspace);
    assert_eq!(statuses.get("project-a").map(String::as_str), Some("skipped"));
    assert_eq!(statuses.get("project-a#build").map(String::as_str), Some("failure"));

    drop(root);
}

/// When no selected project has the requested script, the run errors
/// before the tasks `dependsOn` pulled in get to run their side effects.
#[test]
fn missing_requested_script_errors_before_upstream_tasks_run() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[(
            "project-a",
            json!({
                "name": "project-a",
                "version": "1.0.0",
                "scripts": { "codegen": "echo codegen >> ../order.log" },
            }),
        )],
    );
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - project-a\ntasks:\n  build:\n    dependsOn: ['codegen']\n",
    )
    .expect("write workspace settings");

    let output = pacquet.with_args(["-r", "run", "build"]).output().expect("run recursive script");
    assert!(!output.status.success(), "a script nothing declares must fail the run");
    assert!(String::from_utf8_lossy(&output.stderr).contains("RECURSIVE_RUN_NO_SCRIPT"));
    assert!(!workspace.join("order.log").exists(), "the pulled-in task must not have run");

    drop(root);
}

#[test]
fn regexp_selected_empty_script_errors_before_upstream_tasks_run() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[(
            "project-a",
            json!({
                "name": "project-a",
                "version": "1.0.0",
                "scripts": { "build:empty": "", "codegen": "echo codegen >> ../order.log" },
            }),
        )],
    );
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - project-a\ntasks:\n  '/^build:/':\n    dependsOn: ['codegen']\n",
    )
    .expect("write workspace settings");

    let output =
        pacquet.with_args(["-r", "run", "/^build:/"]).output().expect("run recursive script");
    eprintln!("STATUS: {}", output.status);
    assert!(!output.status.success(), "an empty selected script must fail the run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(stderr.contains("RECURSIVE_RUN_NO_SCRIPT"));
    let order_log_exists = workspace.join("order.log").exists();
    eprintln!("ORDER LOG EXISTS: {order_log_exists}");
    assert!(!order_log_exists, "the pulled-in task must not have run");

    drop(root);
}

/// `ignoreWorkspaceCycles: true` downgrades the task-cycle error to a
/// warning: the cycle's members run in an arbitrary order relative to each
/// other and the run completes.
#[test]
fn ignore_workspace_cycles_downgrades_the_task_cycle_error_to_a_warning() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let cyclic = |name: &str, dependency: &str| {
        json!({
            "name": name,
            "version": "1.0.0",
            "dependencies": { dependency: "workspace:*" },
            "scripts": { "build": format!("echo {name} >> ../order.log") },
        })
    };
    write_workspace(
        &workspace,
        &[
            ("project-a", cyclic("project-a", "project-b")),
            ("project-b", cyclic("project-b", "project-a")),
        ],
    );
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - project-a\n  - project-b\nignoreWorkspaceCycles: true\n",
    )
    .expect("write workspace settings");

    let output = pacquet
        .with_args(["--workspace-concurrency=1", "-r", "run", "build"])
        .output()
        .expect("run recursive script");
    assert!(output.status.success(), "the tolerated cycle must not fail the run: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("[WARN] The tasks form a dependency cycle"),
        "the tolerated cycle must still be reported: {stdout}",
    );
    let order = fs::read_to_string(workspace.join("order.log")).expect("read order log");
    let mut lines: Vec<&str> = order.lines().collect();
    lines.sort_unstable();
    assert_eq!(lines, ["project-a", "project-b"], "both cycle members must run");

    drop(root);
}
