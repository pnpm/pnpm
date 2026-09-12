use super::{
    CommandExtra, CommandTempCwd, Value, build_writes_marker, echoes_ok, fs, json, sorted_lines,
    summary_statuses, write_workspace,
};
use assert_cmd::assert::OutputAssertExt;

/// `pacquet -r run` with no script name surfaces the
/// `ERR_PNPM_SCRIPT_NAME_IS_REQUIRED` typed error variant.
#[test]
fn recursive_run_without_script_name_errors_with_script_name_is_required() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("project-1", build_writes_marker("project-1")),
            ("project-2", build_writes_marker("project-2")),
        ],
    );

    let output = pacquet.with_arg("-r").with_arg("run").output().expect("spawn pacquet");
    assert!(!output.status.success(), "missing script name in recursive mode must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_SCRIPT_NAME_IS_REQUIRED"),
        "stderr should carry the script-name-required code, got: {stderr}",
    );

    drop(root);
}

/// Port of upstream's `testPattern is respected by the test script`
/// (`pnpm/test/monorepo/index.ts`): with `testPattern` in
/// `pnpm-workspace.yaml`, a `...[<since>]` filter selects a project
/// whose only changes match the pattern (project-2) without its
/// dependents (project-1, project-3), while a source-changed project
/// (project-4) is selected normally.
#[test]
fn test_pattern_from_workspace_yaml_is_respected_by_the_test_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let test_writes_marker = |name: &str, dependencies: Value| {
        json!({
            "name": name,
            "version": "1.0.0",
            "dependencies": dependencies,
            "scripts": { "test": "touch tested.txt" },
        })
    };
    write_workspace(
        &workspace,
        &[
            (
                "project-1",
                test_writes_marker(
                    "project-1",
                    json!({ "project-2": "workspace:*", "project-3": "workspace:*" }),
                ),
            ),
            ("project-2", test_writes_marker("project-2", json!({}))),
            ("project-3", test_writes_marker("project-3", json!({ "project-2": "workspace:*" }))),
            ("project-4", test_writes_marker("project-4", json!({}))),
        ],
    );

    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&workspace)
            .output()
            .expect("spawn git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr),
        );
    };
    let remote = root.path().join("remote");
    fs::create_dir_all(&remote).expect("create remote dir");
    git(&["init", "--initial-branch=main"]);
    git(&["config", "user.email", "x@y.z"]);
    git(&["config", "user.name", "xyz"]);
    git(&["init", "--bare", &remote.to_string_lossy()]);
    git(&["add", "."]);
    git(&["commit", "-m", "init", "--no-gpg-sign"]);
    git(&["remote", "add", "origin", &remote.to_string_lossy()]);
    git(&["push", "-u", "origin", "main"]);

    fs::write(workspace.join("project-2").join("file.js"), "").expect("write changed file");
    fs::write(workspace.join("project-4").join("different-pattern.js"), "")
        .expect("write changed file");
    let workspace_yaml = "packages:\n  - project-1\n  - project-2\n  - project-3\n  - project-4\ntestPattern:\n  - '*/file.js'\n";
    fs::write(workspace.join("pnpm-workspace.yaml"), workspace_yaml)
        .expect("write pnpm-workspace.yaml");
    git(&["add", "."]);
    git(&["commit", "-m", "changes", "--no-gpg-sign"]);

    pacquet.with_arg("--filter").with_arg("...[origin/main]").with_arg("test").assert().success();

    for name in ["project-2", "project-4"] {
        assert!(
            workspace.join(name).join("tested.txt").exists(),
            "{name} changed, so its test script should run",
        );
    }
    for name in ["project-1", "project-3"] {
        assert!(
            !workspace.join(name).join("tested.txt").exists(),
            "{name} depends on project-2 whose only change matches testPattern, so it must not run",
        );
    }

    drop(root);
}

/// A `/pattern/` selector runs every matching script in every selected
/// project, not just one script per project.
#[test]
fn recursive_run_executes_every_script_matching_a_regexp_selector() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            (
                "both",
                json!({
                    "name": "both",
                    "version": "1.0.0",
                    "scripts": {
                        "build:backend": "touch backend.txt",
                        "build:frontend": "touch frontend.txt",
                        "test": "touch test.txt",
                    },
                }),
            ),
            (
                "neither",
                json!({
                    "name": "neither",
                    "version": "1.0.0",
                    "scripts": { "test": "touch test.txt" },
                }),
            ),
        ],
    );

    pacquet
        .with_args(["-r", "run", "--report-summary", "/^build:(backend|frontend)$/"])
        .assert()
        .success();

    assert!(workspace.join("both").join("backend.txt").exists());
    assert!(workspace.join("both").join("frontend.txt").exists());
    assert!(!workspace.join("both").join("test.txt").exists());

    let statuses = summary_statuses(&workspace);
    assert_eq!(statuses.get("both").map(String::as_str), Some("passed"));
    assert_eq!(statuses.get("neither").map(String::as_str), Some("skipped"), "{statuses:?}");

    drop(root);
}

/// Port of upstream's `pnpm run with --stream should prefix output`
/// (`pnpm/test/monorepo/index.ts`).
#[test]
fn stream_prefixes_recursive_script_output_with_the_project() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[("project-1", echoes_ok("project-1")), ("project-2", echoes_ok("project-2"))],
    );

    let output = pacquet
        .with_args(["--stream", "--config.verify-deps-before-run=false", "-r", "run", "test"])
        .output()
        .expect("run test");
    assert!(output.status.success(), "streamed run failed: {output:?}");
    assert_eq!(
        sorted_lines(&output.stdout),
        [
            "Scope: all 2 workspace projects",
            "project-1 test$ echo OK",
            "project-1 test: Done",
            "project-1 test: OK",
            "project-2 test$ echo OK",
            "project-2 test: Done",
            "project-2 test: OK",
        ],
    );

    drop(root);
}

/// Port of upstream's `run --reporter-hide-prefix should hide prefix`
/// (`pnpm/test/monorepo/index.ts`): only the script's own output loses
/// the prefix — the command echo and the `Done` line keep theirs.
#[test]
fn reporter_hide_prefix_drops_the_prefix_from_streamed_script_output() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[("project-1", echoes_ok("project-1")), ("project-2", echoes_ok("project-2"))],
    );

    let output = pacquet
        .with_args([
            "--stream",
            "--reporter-hide-prefix",
            "--config.verify-deps-before-run=false",
            "-r",
            "run",
            "test",
        ])
        .output()
        .expect("run test");
    assert!(output.status.success(), "streamed run failed: {output:?}");
    assert_eq!(
        sorted_lines(&output.stdout),
        [
            "OK",
            "OK",
            "Scope: all 2 workspace projects",
            "project-1 test$ echo OK",
            "project-1 test: Done",
            "project-2 test$ echo OK",
            "project-2 test: Done",
        ],
    );

    drop(root);
}

/// `--parallel` expands to `--stream` in pnpm's `run` shorthand table,
/// so a parallel run is prefixed even without the flag — otherwise the
/// interleaved output of concurrent projects is unattributable.
#[test]
fn parallel_implies_stream() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[("project-1", echoes_ok("project-1")), ("project-2", echoes_ok("project-2"))],
    );

    let output = pacquet
        .with_args(["--parallel", "--config.verify-deps-before-run=false", "run", "test"])
        .output()
        .expect("run test");
    assert!(output.status.success(), "parallel run failed: {output:?}");
    assert_eq!(
        sorted_lines(&output.stdout),
        [
            "Scope: all 2 workspace projects",
            "project-1 test$ echo OK",
            "project-1 test: Done",
            "project-1 test: OK",
            "project-2 test$ echo OK",
            "project-2 test: Done",
            "project-2 test: OK",
        ],
    );

    drop(root);
}

/// Without `--stream` the children inherit the terminal, so their output
/// carries no prefix at all.
#[test]
fn recursive_run_inherits_stdio_without_stream() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    // A dependency chain: the graph forces the two scripts to run one
    // after another, so at most one is ever in flight and output stays
    // inherited.
    let mut dependent = echoes_ok("project-2");
    dependent["dependencies"] = json!({ "project-1": "workspace:*" });
    write_workspace(&workspace, &[("project-1", echoes_ok("project-1")), ("project-2", dependent)]);

    let output = pacquet
        .with_args(["--config.verify-deps-before-run=false", "-r", "run", "test"])
        .output()
        .expect("run test");
    assert!(output.status.success(), "recursive run failed: {output:?}");
    assert_eq!(sorted_lines(&output.stdout), ["OK", "OK", "Scope: all 2 workspace projects"]);

    drop(root);
}

/// Two independent projects can have their scripts in flight at once, so
/// output is piped and prefixed even without `--stream` — inherited
/// terminal output would interleave them mid-line.
#[test]
fn recursive_run_pipes_stdio_when_tasks_can_interleave() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[("project-1", echoes_ok("project-1")), ("project-2", echoes_ok("project-2"))],
    );

    let output = pacquet
        .with_args(["--config.verify-deps-before-run=false", "-r", "run", "test"])
        .output()
        .expect("run test");
    assert!(output.status.success(), "recursive run failed: {output:?}");
    assert_eq!(
        sorted_lines(&output.stdout),
        [
            "Scope: all 2 workspace projects",
            "project-1 test$ echo OK",
            "project-1 test: Done",
            "project-1 test: OK",
            "project-2 test$ echo OK",
            "project-2 test: Done",
            "project-2 test: OK",
        ],
    );

    drop(root);
}

/// Non-UTF-8 output must not stall the pump: the child keeps writing past
/// the bad bytes, so a reader that gave up there would leave it blocked on
/// a full pipe with the run waiting on it forever.
#[test]
fn streamed_output_survives_non_utf8_bytes() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    // A lone 0xff is invalid UTF-8 in any position. The padding after it
    // is what fills the pipe if the pump stops draining.
    write_workspace(
        &workspace,
        &[(
            "project-1",
            json!({
                "name": "project-1",
                "version": "1.0.0",
                "scripts": {
                    "test": r#"node -e "process.stdout.write(Buffer.from([0xff])); console.log('x'.repeat(200000)); console.log('done')""#,
                },
            }),
        )],
    );

    let output = pacquet
        .with_args(["--stream", "--config.verify-deps-before-run=false", "-r", "run", "test"])
        .output()
        .expect("run test");
    assert!(output.status.success(), "streamed run failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("project-1 test: done"), "the pump stopped early: {stdout}");

    drop(root);
}

/// `--aggregate-output` withholds each project's streamed lines until it
/// exits, so a slow project cannot interleave into a fast one's block.
#[test]
fn aggregate_output_keeps_each_project_in_one_block() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    // `node -e` rather than `sleep`, whose fractional argument is a GNU
    // extension. project-2 finishes well inside project-1's gap, so
    // without aggregation its lines would land between project-1's two.
    let prints_two_lines_apart = |name: &str, delay: u32| {
        json!({
            "name": name,
            "version": "1.0.0",
            "scripts": {
                "test": format!(
                    r#"node -e "console.log('first'); setTimeout(() => console.log('second'), {delay})""#,
                ),
            },
        })
    };
    write_workspace(
        &workspace,
        &[
            ("project-1", prints_two_lines_apart("project-1", 2000)),
            ("project-2", prints_two_lines_apart("project-2", 0)),
        ],
    );

    let output = pacquet
        .with_args([
            "--parallel",
            "--aggregate-output",
            "--config.verify-deps-before-run=false",
            "run",
            "test",
        ])
        .output()
        .expect("run test");
    assert!(output.status.success(), "aggregated run failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("STDOUT:\n{stdout}\n");
    // The faster project finishes first, and each project's four lines
    // land together.
    let blocks = stdout.trim().split("project-1 test$").collect::<Vec<_>>();
    assert_eq!(blocks.len(), 2, "project-1's block must be contiguous: {stdout}");
    assert!(
        !blocks[1].contains("project-2"),
        "project-2 must have flushed before project-1 started printing: {stdout}",
    );

    drop(root);
}

/// `--use-stderr` moves the reporter's own output to stderr, leaving
/// stdout to the command — here, the scripts' streamed lines still reach
/// stderr with it, since the reporter is what prints them.
#[test]
fn use_stderr_diverts_reporter_output() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &[("project-1", echoes_ok("project-1"))]);

    let output = pacquet
        .with_args([
            "--use-stderr",
            "--stream",
            "--config.verify-deps-before-run=false",
            "-r",
            "run",
            "test",
        ])
        .output()
        .expect("run test");
    assert!(output.status.success(), "run failed: {output:?}");
    assert_eq!(sorted_lines(&output.stdout), Vec::<String>::new());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("project-1 test: OK"),
        "the reporter must have written to stderr: {output:?}",
    );

    drop(root);
}
