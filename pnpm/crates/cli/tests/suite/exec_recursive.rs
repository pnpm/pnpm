//! Recursive-exec integration tests. They drive the commands through a
//! POSIX shell (`touch`, `sh -c`), so the whole file is gated to Unix —
//! same as the recursive-run tests.
#![cfg(unix)]

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, command_env::CommandTestExt};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

/// Write a `pnpm-workspace.yaml` listing `names` as packages, plus a
/// `package.json` per name under its own subdirectory of `workspace`.
fn write_workspace(workspace: &Path, names: &[&str]) {
    let packages = names.iter().map(|name| format!("  - {name}")).collect::<Vec<_>>();
    let workspace_yaml = format!("packages:\n{}\n", packages.join("\n"));
    fs::write(workspace.join("pnpm-workspace.yaml"), workspace_yaml)
        .expect("write pnpm-workspace.yaml");
    for name in names {
        let dir = workspace.join(name);
        fs::create_dir_all(&dir).expect("create project dir");
        let manifest = json!({ "name": name, "version": "1.0.0" });
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

fn write_concurrency_probe(workspace: &Path) {
    fs::write(
        workspace.join("track-concurrency.sh"),
        r#"marker=../active-$(basename "$PWD")
mkdir "$marker"
sleep 0.2
set -- ../active-*
[ -e "$1" ] || set --
[ "$#" -ge 2 ] && touch ../saw-parallel
[ "$#" -gt 2 ] && touch ../exceeded-concurrency
sleep 0.2
rmdir "$marker"
"#,
    )
    .expect("write concurrency probe");
}

fn process_group_probe() -> &'static str {
    r#"child_group=$(ps -o pgid= -p $$ | tr -d ' ')
parent_group=$(ps -o pgid= -p $PPID | tr -d ' ')
printf "%s %s\n" "$child_group" "$parent_group" > ../process-groups.txt"#
}

/// `pacquet -r exec <command>` runs the command once in every workspace
/// project, each with cwd == its own package root — a relative `touch`
/// lands a marker inside each package directory.
#[test]
fn recursive_exec_runs_command_in_every_project() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["project-1", "project-2", "project-3"]);

    pacquet
        .with_arg("-r")
        .with_arg("exec")
        .with_arg("touch")
        .with_arg("ran.txt")
        .assert()
        .success();

    for name in ["project-1", "project-2", "project-3"] {
        assert!(
            workspace.join(name).join("ran.txt").exists(),
            "{name} should have run the command in its own directory",
        );
    }

    drop(root);
}

/// A single filtered command cannot run alongside a sibling, so it must
/// stay in pacquet's own process group: a child moved into its own group
/// is stopped the moment it reads from the terminal.
#[test]
fn filtered_exec_keeps_single_command_in_foreground_process_group() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["project-1", "project-2"]);

    pacquet
        .with_args(["--filter", "project-1", "exec", "sh", "-c", process_group_probe()])
        .assert()
        .success();

    let groups =
        fs::read_to_string(workspace.join("process-groups.txt")).expect("read process groups");
    let mut fields = groups.split_whitespace();
    let child_group = fields.next().expect("child process group");
    let parent_group = fields.next().expect("parent process group");
    assert_eq!(
        child_group, parent_group,
        "the child must share pacquet's process group to keep reading the terminal",
    );

    drop(root);
}

#[test]
fn recursive_exec_respects_workspace_concurrency() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["project-1", "project-2", "project-3"]);
    write_concurrency_probe(&workspace);

    pacquet
        .with_args(["--workspace-concurrency=2", "-r", "exec", "sh", "../track-concurrency.sh"])
        .assert()
        .success();

    assert!(workspace.join("saw-parallel").exists(), "two commands should overlap");
    assert!(
        !workspace.join("exceeded-concurrency").exists(),
        "no more than two commands should overlap",
    );

    drop(root);
}

#[test]
fn recursive_exec_no_sort_makes_reverse_and_resume_no_ops() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["z-first", "m-middle", "a-last"]);

    pacquet
        .with_args([
            "--workspace-concurrency=1",
            "--no-sort",
            "--reverse",
            "--resume-from=m-middle",
            "-r",
            "exec",
            "-c",
            r#"echo "$(basename "$PWD")" >> ../order.log"#,
        ])
        .assert()
        .success();

    // `--no-sort` disregards ordering entirely, so there is no graph for
    // `--reverse` to turn around or for `--resume-from` to skip the
    // anchor's dependencies in — both are no-ops, exactly as in pnpm.
    let order = fs::read_to_string(workspace.join("order.log")).expect("read order log");
    assert_eq!(order, "a-last\nm-middle\nz-first\n");

    drop(root);
}

#[test]
fn parallel_recursive_exec_has_no_workspace_concurrency_cap() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["project-1", "project-2", "project-3"]);
    write_concurrency_probe(&workspace);

    pacquet.with_args(["--parallel", "exec", "sh", "../track-concurrency.sh"]).assert().success();

    assert!(
        workspace.join("exceeded-concurrency").exists(),
        "--parallel should start all three commands together",
    );

    drop(root);
}

/// `pacquet -r --filter <name> exec <command>` runs the command only in
/// the `--filter`-selected project. Threads `config.filter` through the
/// recursive exec dispatch.
#[test]
fn recursive_exec_filter_selects_only_matching_project() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["project-1", "project-2", "project-3"]);

    pacquet
        .with_arg("-r")
        .with_arg("--filter")
        .with_arg("project-1")
        .with_arg("exec")
        .with_arg("touch")
        .with_arg("ran.txt")
        .assert()
        .success();

    assert!(
        workspace.join("project-1").join("ran.txt").exists(),
        "the selected project-1 should run the command",
    );
    for name in ["project-2", "project-3"] {
        assert!(
            !workspace.join(name).join("ran.txt").exists(),
            "{name} is not selected by --filter and must not run",
        );
    }

    drop(root);
}

/// A bare `--filter` (no `-r`) enters recursive mode CLI-wide, matching
/// pnpm's `parse-cli-args` promotion: the command runs only in the
/// selected project even though `-r` was never passed.
#[test]
fn filter_without_recursive_flag_enters_recursive_exec() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["project-1", "project-2"]);

    pacquet
        .with_arg("--filter")
        .with_arg("project-1")
        .with_arg("exec")
        .with_arg("touch")
        .with_arg("ran.txt")
        .assert()
        .success();

    assert!(
        workspace.join("project-1").join("ran.txt").exists(),
        "the selected project-1 should run the command",
    );
    assert!(
        !workspace.join("project-2").join("ran.txt").exists(),
        "a bare --filter (no -r) should still scope the exec to the selection",
    );

    drop(root);
}

/// A `[<since>]` changed-packages selector scopes a recursive `exec` to
/// the projects the git diff touches.
#[test]
fn recursive_exec_diff_selector_selects_changed_projects() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["project-1", "project-2"]);
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
    git(&["init", "--initial-branch=main"]);
    git(&["config", "user.email", "x@y.z"]);
    git(&["config", "user.name", "xyz"]);
    git(&["add", "."]);
    git(&["commit", "-m", "base", "--no-gpg-sign"]);
    fs::write(workspace.join("project-1").join("changed.js"), "").expect("write changed file");
    git(&["add", "."]);
    git(&["commit", "-m", "change project-1", "--no-gpg-sign"]);

    pacquet
        .with_arg("-r")
        .with_arg("--filter")
        .with_arg("[HEAD~1]")
        .with_arg("exec")
        .with_arg("touch")
        .with_arg("ran.txt")
        .assert()
        .success();

    assert!(
        workspace.join("project-1").join("ran.txt").exists(),
        "the changed project-1 should run the command",
    );
    assert!(
        !workspace.join("project-2").join("ran.txt").exists(),
        "the unchanged project-2 must stay outside the selection",
    );

    drop(root);
}

/// A `--filter` that matches no project is a no-op: recursive exec exits
/// 0 and writes no summary even with `--report-summary`, matching pnpm's
/// main-dispatch exit-0 for an empty selection — rather than erroring on
/// `--resume-from` or emitting an empty summary.
#[test]
fn recursive_exec_filter_no_match_is_a_noop() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["project-1", "project-2"]);

    pacquet
        .with_arg("-r")
        .with_arg("--filter")
        .with_arg("does-not-exist")
        .with_arg("exec")
        .with_arg("--report-summary")
        .with_arg("touch")
        .with_arg("ran.txt")
        .assert()
        .success();

    for name in ["project-1", "project-2"] {
        assert!(
            !workspace.join(name).join("ran.txt").exists(),
            "no project is selected, so {name} should not run",
        );
    }
    assert!(
        !workspace.join("pnpm-exec-summary.json").exists(),
        "an empty selection should not write a summary file",
    );

    drop(root);
}

/// `--report-summary` writes `pnpm-exec-summary.json` with a `passed`
/// entry for every project.
#[test]
fn recursive_exec_report_summary_records_every_package_status() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["project-1", "project-2"]);

    pacquet
        .with_arg("-r")
        .with_arg("exec")
        .with_arg("--report-summary")
        .with_arg("true")
        .assert()
        .success();

    let statuses = summary_statuses(&workspace);
    assert_eq!(statuses.get("project-1").map(String::as_str), Some("passed"));
    assert_eq!(statuses.get("project-2").map(String::as_str), Some("passed"));

    drop(root);
}

#[test]
fn recursive_exec_bail_cancels_in_flight_processes() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["a-slow-1", "b-fails", "c-slow-2", "z-queued"]);

    let start = Instant::now();
    let output = pacquet
        .with_args([
            "--workspace-concurrency=3",
            "--no-sort",
            "--report-summary",
            "-r",
            "exec",
            "-c",
            r#"name=$(basename "$PWD"); if [ "$name" = a-slow-1 ] || [ "$name" = c-slow-2 ]; then touch ran.txt; sleep 5; elif [ "$name" = b-fails ]; then i=0; while [ ! -f ../a-slow-1/ran.txt ] || [ ! -f ../c-slow-2/ran.txt ]; do i=$((i + 1)); [ $i -lt 500 ] || exit 2; sleep 0.01; done; exit 1; else touch ran.txt; fi"#,
        ])
        .output()
        .expect("spawn pacquet");
    let elapsed = start.elapsed();
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(!output.status.success(), "the failing project should fail the exec");
    eprintln!("recursive exec elapsed: {elapsed:?}");
    assert!(
        elapsed < Duration::from_secs(4),
        "bail should interrupt the five-second in-flight commands",
    );

    let statuses = summary_statuses(&workspace);
    dbg!(&statuses);
    assert_eq!(statuses.get("a-slow-1").map(String::as_str), Some("running"));
    assert_eq!(statuses.get("b-fails").map(String::as_str), Some("failure"));
    assert_eq!(statuses.get("c-slow-2").map(String::as_str), Some("running"));
    assert_eq!(statuses.get("z-queued").map(String::as_str), Some("queued"));
    assert!(!workspace.join("z-queued").join("ran.txt").exists());

    drop(root);
}

/// With `--no-bail`, a failing command runs in every project and the
/// invocation still ends with a non-zero exit (the recursive-fail error).
#[test]
fn recursive_exec_no_bail_runs_all_then_fails() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["project-1", "project-2", "project-3"]);

    let output = pacquet
        .with_arg("-r")
        .with_arg("exec")
        .with_arg("--no-bail")
        .with_arg("-c")
        .with_arg("touch ran.txt && exit 1")
        .output()
        .expect("spawn pacquet -r exec");

    assert!(!output.status.success(), "a failing command must surface a non-zero exit");
    for name in ["project-1", "project-2", "project-3"] {
        assert!(
            workspace.join(name).join("ran.txt").exists(),
            "--no-bail should still run {name} despite earlier failures",
        );
    }

    drop(root);
}

/// With one workspace task in flight, execution stops at the first failing
/// project, so at least one project never runs.
#[test]
fn recursive_exec_bail_stops_at_first_failure() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["project-1", "project-2", "project-3"]);

    let output = pacquet
        .with_arg("--workspace-concurrency=1")
        .with_arg("-r")
        .with_arg("exec")
        .with_arg("-c")
        .with_arg("touch ran.txt && exit 1")
        .output()
        .expect("spawn pacquet -r exec");

    assert!(!output.status.success(), "a failing command must surface a non-zero exit");
    let ran = ["project-1", "project-2", "project-3"]
        .into_iter()
        .filter(|name| workspace.join(name).join("ran.txt").exists())
        .count();
    assert!(ran < 3, "bail should stop before every project runs, but {ran}/3 ran");

    drop(root);
}

/// A settings-only `pnpm-workspace.yaml` (no `packages:`) enumerates the
/// root project only; it must not recursively pick up vendored fixture
/// packages.
#[test]
fn recursive_exec_settings_only_workspace_enumerates_root_only() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "root", "version": "1.0.0" }).to_string(),
    )
    .expect("write root package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "allowBuilds:\n  esbuild: false\n")
        .expect("write settings-only workspace manifest");

    let nested = workspace.join("test-e2e/fixtures/vendor/preact/.cache/10.10.2");
    fs::create_dir_all(&nested).expect("create vendored package dir");
    fs::write(
        nested.join("package.json"),
        json!({ "name": "preact", "version": "10.10.2" }).to_string(),
    )
    .expect("write vendored package.json");

    pacquet
        .with_arg("-r")
        .with_arg("exec")
        .with_arg("touch")
        .with_arg("ran.txt")
        .assert()
        .success();

    assert!(workspace.join("ran.txt").exists(), "root project should run the command");
    assert!(
        !nested.join("ran.txt").exists(),
        "settings-only workspace manifests must not recursively enumerate vendored packages",
    );

    drop(root);
}

/// Port of upstream's `pnpm exec --recursive --no-reporter-hide-prefix
/// prints prefixes` (`exec/commands/test/exec.logs.ts`). `exec` labels
/// its output with the package name and a `(exec)` stage.
#[test]
fn no_reporter_hide_prefix_labels_each_project() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["project-1", "project-2"]);

    let output = pacquet
        .with_args([
            "-r",
            "--no-reporter-hide-prefix",
            "--config.verify-deps-before-run=false",
            "exec",
            "echo",
            "hello",
        ])
        .output()
        .expect("run exec");
    assert!(output.status.success(), "prefixed exec failed: {output:?}");
    assert_eq!(
        sorted_lines(&output.stdout),
        [
            "project-1 (exec): Done",
            "project-1 (exec): hello",
            "project-2 (exec): Done",
            "project-2 (exec): hello",
        ],
    );

    drop(root);
}

/// A `{<dir>}` selector matches the directory as a glob, so it selects the
/// project living in that directory and not the ones nested below it.
#[test]
fn a_dir_selector_selects_the_project_in_that_dir() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["nested", "nested/inner"]);

    pacquet
        .with_arg("-r")
        .with_arg("--filter")
        .with_arg("{nested}")
        .with_arg("exec")
        .with_arg("touch")
        .with_arg("ran.txt")
        .assert()
        .success();

    assert!(workspace.join("nested/ran.txt").exists(), "the project at {{nested}} should run");
    assert!(
        !workspace.join("nested/inner/ran.txt").exists(),
        "a project below {{nested}} is not selected by the glob match",
    );

    drop(root);
}

/// Port of upstream's `pnpm exec --recursive does not print prefixes by
/// default`: unlike `run`, `exec` inherits the terminal unless the user
/// turns the hiding off explicitly. `--stream` does not change that.
#[test]
fn recursive_exec_inherits_stdio_by_default() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["project-1", "project-2"]);

    for extra in [[].as_slice(), ["--stream"].as_slice(), ["--reporter-hide-prefix"].as_slice()] {
        let mut args = vec!["-r", "--config.verify-deps-before-run=false"];
        args.extend_from_slice(extra);
        args.extend_from_slice(&["exec", "echo", "hello"]);
        let output = Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
            .with_current_dir(&workspace)
            .without_ambient_pnpm_config()
            .with_args(args)
            .output()
            .expect("run exec");
        assert!(output.status.success(), "exec failed with {extra:?}: {output:?}");
        assert_eq!(sorted_lines(&output.stdout), ["hello", "hello"]);
    }

    drop(root);
}

/// Sorted so a test does not depend on the order the projects finish in.
fn sorted_lines(stdout: &[u8]) -> Vec<String> {
    let stdout = String::from_utf8_lossy(stdout);
    eprintln!("STDOUT:\n{stdout}\n");
    let mut lines = stdout.trim().lines().map(str::to_string).collect::<Vec<_>>();
    lines.sort();
    lines
}

/// Under `legacyDirFiltering` the selector matches by subtree instead: it
/// names the projects strictly below the directory, and not the project in
/// the directory itself.
#[test]
fn legacy_dir_filtering_selects_the_subtree_below_the_dir() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["nested", "nested/inner"]);
    let workspace_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .expect("read pnpm-workspace.yaml");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("{workspace_yaml}legacyDirFiltering: true\n"),
    )
    .expect("write pnpm-workspace.yaml");

    pacquet
        .with_arg("-r")
        .with_arg("--filter")
        .with_arg("{nested}")
        .with_arg("exec")
        .with_arg("touch")
        .with_arg("ran.txt")
        .assert()
        .success();

    assert!(
        workspace.join("nested/inner/ran.txt").exists(),
        "the project below {{nested}} should run under legacyDirFiltering",
    );
    assert!(
        !workspace.join("nested/ran.txt").exists(),
        "the subtree match excludes the directory it starts from",
    );

    drop(root);
}

/// The `!{<workspace-root>}` selector a recursive `run` / `exec` generates
/// is pnpm's own, so `legacyDirFiltering` must not reach it: read as a
/// subtree match it would name every project below the root and leave the
/// root alone selected, which is the opposite of what it is for.
#[test]
fn legacy_dir_filtering_leaves_the_generated_root_exclusion_alone() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["project-1"]);
    let workspace_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .expect("read pnpm-workspace.yaml");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("{workspace_yaml}legacyDirFiltering: true\n"),
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "root", "version": "1.0.0" }).to_string(),
    )
    .expect("write the root package.json");

    pacquet
        .with_args(["-r", "--config.verify-deps-before-run=false", "exec", "touch", "ran.txt"])
        .assert()
        .success();

    assert!(
        workspace.join("project-1/ran.txt").exists(),
        "the workspace projects should run under legacyDirFiltering",
    );
    assert!(
        !workspace.join("ran.txt").exists(),
        "the workspace root is still excluded from a recursive exec",
    );

    drop(root);
}

/// The `{<workspace-root>}` selector `--workspace-root` generates is pnpm's
/// own as well: read as a subtree match it would name every project below
/// the root and run the command everywhere but the root.
#[test]
fn legacy_dir_filtering_leaves_the_generated_root_inclusion_alone() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["project-1"]);
    let workspace_yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .expect("read pnpm-workspace.yaml");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("{workspace_yaml}legacyDirFiltering: true\n"),
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "root", "version": "1.0.0" }).to_string(),
    )
    .expect("write the root package.json");

    pacquet
        .with_args([
            "-r",
            "--workspace-root",
            "--config.verify-deps-before-run=false",
            "exec",
            "touch",
            "ran.txt",
        ])
        .assert()
        .success();

    assert!(
        workspace.join("ran.txt").exists(),
        "--workspace-root should select the workspace root under legacyDirFiltering",
    );
    assert!(
        !workspace.join("project-1/ran.txt").exists(),
        "--workspace-root selects the root alone, not the projects below it",
    );

    drop(root);
}

/// Write projects with explicit manifests (workspace dependencies and
/// all), for the graph-shaped scenarios below.
fn write_workspace_manifests(workspace: &Path, manifests: &[(&str, Value)]) {
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

/// The failure that blocked the dependent is already counted; the skipped
/// dependent must not turn one failure into two.
#[test]
fn recursive_exec_no_bail_skips_dependents_of_a_failed_command() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace_manifests(
        &workspace,
        &[
            (
                "project-a",
                json!({
                    "name": "project-a",
                    "version": "1.0.0",
                    "dependencies": { "project-b": "workspace:*" },
                }),
            ),
            ("project-b", json!({ "name": "project-b", "version": "1.0.0" })),
            ("project-c", json!({ "name": "project-c", "version": "1.0.0" })),
        ],
    );

    let output = pacquet
        .with_args([
            "--no-bail",
            "-r",
            "exec",
            "--report-summary",
            "-c",
            r#"[ "$(basename "$PWD")" != project-b ] && echo "$(basename "$PWD")" >> ../order.log"#,
        ])
        .output()
        .expect("run recursive exec");
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

/// Only the anchor's transitive dependencies are skipped; a project
/// unrelated to the anchor still runs.
#[test]
fn recursive_exec_resume_from_skips_only_the_anchors_dependencies() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace_manifests(
        &workspace,
        &[
            ("project-1", json!({ "name": "project-1", "version": "1.0.0" })),
            (
                "project-2",
                json!({
                    "name": "project-2",
                    "version": "1.0.0",
                    "dependencies": { "project-1": "workspace:*" },
                }),
            ),
            (
                "project-3",
                json!({
                    "name": "project-3",
                    "version": "1.0.0",
                    "dependencies": { "project-1": "workspace:*" },
                }),
            ),
            ("project-4", json!({ "name": "project-4", "version": "1.0.0" })),
        ],
    );

    pacquet
        .with_args([
            "--workspace-concurrency=1",
            "--resume-from=project-3",
            "-r",
            "exec",
            "-c",
            r#"echo "$(basename "$PWD")" >> ../order.log"#,
        ])
        .assert()
        .success();

    let order = fs::read_to_string(workspace.join("order.log")).expect("read order log");
    let mut lines: Vec<&str> = order.lines().collect();
    lines.sort_unstable();
    assert_eq!(lines, ["project-2", "project-3", "project-4"]);

    drop(root);
}

#[test]
fn recursive_exec_resumes_from_exactly_the_projects_that_passed_before_a_failure() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace_manifests(
        &workspace,
        &[
            ("dependency", json!({ "name": "dependency", "version": "1.0.0" })),
            (
                "anchor",
                json!({
                    "name": "anchor",
                    "version": "1.0.0",
                    "dependencies": { "dependency": "workspace:*" },
                }),
            ),
            ("completed", json!({ "name": "completed", "version": "1.0.0" })),
        ],
    );
    fs::write(workspace.join("fail"), "").expect("write failure marker");
    let command = r#"name=$(basename "$PWD"); echo "$name" >> ../order.log; [ "$name" != dependency ] || [ ! -e ../fail ]"#;

    pacquet
        .with_args(["--no-bail", "--workspace-concurrency=1", "-r", "exec", "sh", "-c", command])
        .assert()
        .failure();
    let first_run = fs::read_to_string(workspace.join("order.log")).expect("read first run");
    let mut first_projects: Vec<&str> = first_run.lines().collect();
    first_projects.sort_unstable();
    assert_eq!(first_projects, ["completed", "dependency"]);

    fs::remove_file(workspace.join("fail")).expect("remove failure marker");
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args([
            "--workspace-concurrency=1",
            "--resume-from=anchor",
            "-r",
            "exec",
            "sh",
            "-c",
            command,
        ])
        .assert()
        .success();

    let order = fs::read_to_string(workspace.join("order.log")).expect("read resumed run");
    assert!(order.ends_with("dependency\nanchor\n"), "unfinished dependency must rerun: {order}");
    assert_eq!(order.lines().filter(|project| *project == "completed").count(), 1);

    drop(root);
}
