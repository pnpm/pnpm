//! Recursive-exec integration tests. They drive the commands through a
//! POSIX shell (`touch`, `sh -c`), so the whole file is gated to Unix —
//! same as the recursive-run tests.
#![cfg(unix)]

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{collections::HashMap, fs, path::Path};

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

/// A `[<since>]` changed-packages selector is not supported by pacquet's
/// filter engine yet, so a recursive `exec` surfaces the
/// `UnsupportedDiffSelector` error instead of swallowing it. This
/// exercises the error-propagation (`?`) out of `select_recursive_projects`.
#[test]
fn recursive_exec_diff_selector_is_unsupported() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["project-1"]);

    let output = pacquet
        .with_arg("-r")
        .with_arg("--filter")
        .with_arg("[main]")
        .with_arg("exec")
        .with_arg("true")
        .output()
        .expect("spawn pacquet");
    assert!(!output.status.success(), "a [<since>] diff selector is unsupported and must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Changed-package filter selectors"),
        "stderr should explain the diff selector is unsupported, got: {stderr}",
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

/// Without `--no-bail`, execution stops at the first failing project, so
/// at least one project never runs.
#[test]
fn recursive_exec_bail_stops_at_first_failure() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &["project-1", "project-2", "project-3"]);

    let output = pacquet
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
