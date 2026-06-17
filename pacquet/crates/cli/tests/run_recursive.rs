//! Recursive-run integration tests. The build scripts run through
//! pacquet's `sh -c` executor, so the whole file is gated to Unix —
//! same as the single-package `run` tests.
#![cfg(unix)]

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{collections::HashMap, fs, os::unix::fs::PermissionsExt, path::Path};

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

/// A package whose `build` script writes a marker via a *relative* path
/// (`touch ran.txt`), so it lands in the script's working directory.
/// Tests assert the marker appears under the package's own root, which
/// only holds if each script runs with cwd == its package root rather
/// than the workspace root.
fn build_writes_marker(name: &str) -> Value {
    json!({
        "name": name,
        "version": "1.0.0",
        "scripts": { "build": "touch ran.txt" },
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
            ("project-1", build_writes_marker("project-1")),
            ("project-2", build_writes_marker("project-2")),
            ("project-3", build_writes_marker("project-3")),
        ],
    );

    pacquet.with_arg("-r").with_arg("run").with_arg("build").assert().success();

    for name in ["project-1", "project-2", "project-3"] {
        assert!(
            workspace.join(name).join("ran.txt").exists(),
            "{name} build script should have run from its own package root",
        );
    }
    assert!(
        !workspace.join("ran.txt").exists(),
        "scripts must run from each package root, not the workspace root",
    );

    drop(root);
}

#[test]
fn recursive_run_settings_only_workspace_enumerates_root_only() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "root",
            "version": "1.0.0",
            "scripts": { "build": "touch root-ran.txt" },
        })
        .to_string(),
    )
    .expect("write root package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "allowBuilds:\n  esbuild: false\n")
        .expect("write settings-only workspace manifest");

    let nested = workspace.join("test-e2e/fixtures/vendor/preact/.cache/10.10.2");
    fs::create_dir_all(&nested).expect("create vendored package dir");
    fs::write(
        nested.join("package.json"),
        json!({
            "name": "preact",
            "version": "10.10.2",
            "scripts": { "build": "touch vendored-ran.txt" },
        })
        .to_string(),
    )
    .expect("write vendored package.json");

    pacquet.with_arg("-r").with_arg("run").with_arg("build").assert().success();

    assert!(workspace.join("root-ran.txt").exists(), "root build script should run");
    assert!(
        !nested.join("vendored-ran.txt").exists(),
        "settings-only workspace manifests must not recursively enumerate vendored packages",
    );

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
        let mut manifest = build_writes_marker(name);
        manifest["dependencies"] = json!({ "project-1": "1" });
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

/// An unknown `--resume-from` package fails with pnpm's
/// `ERR_PNPM_RESUME_FROM_NOT_FOUND`. Ports the error path of pnpm's
/// `getResumedPackageChunks`.
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
            ("project-1", build_writes_marker("project-1")),
            ("project-2", build_writes_marker("project-2")),
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
    write_workspace(&workspace, &[("project-1", build_writes_marker("project-1"))]);

    pacquet
        .with_arg("-r")
        .with_arg("run")
        .with_arg("--if-present")
        .with_arg("lint")
        .assert()
        .success();

    drop(root);
}

/// Recursive `run` must resolve each package's `node_modules/.bin` on
/// PATH so locally-installed bins (e.g. `tsc`, `eslint`) work — pnpm's
/// `runLifecycleHook` (runRecursive.ts:124-149) sets this up for every
/// project. Without it, `pacquet -r run build` would fail with
/// `command not found` for any bare bin name living under `.bin`.
#[test]
fn recursive_run_resolves_local_bin_on_path_per_project() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[(
            "pkg-with-local-bin",
            json!({
                "name": "pkg-with-local-bin",
                "version": "1.0.0",
                "scripts": { "build": "say-hi" },
            }),
        )],
    );
    let pkg_root = workspace.join("pkg-with-local-bin");
    let bin_dir = pkg_root.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create node_modules/.bin");
    let script_path = bin_dir.join("say-hi");
    fs::write(&script_path, "#!/bin/sh\ntouch hi.txt\n").expect("write bin");
    let mut perms = fs::metadata(&script_path).expect("stat").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).expect("chmod +x");

    pacquet.with_arg("-r").with_arg("run").with_arg("build").assert().success();
    assert!(
        pkg_root.join("hi.txt").exists(),
        "recursive run should resolve `say-hi` from the package's node_modules/.bin",
    );

    drop(root);
}

/// `pnpm -r run <name>` skips a project whose `<name>` script body is
/// the empty string. pnpm's `runRecursive.ts:107` gates on
/// `!manifest.scripts[name]` (empty string is falsy in JS); pacquet
/// has to mirror that explicitly because `manifest.script` returns
/// `Some("")`.
#[test]
fn recursive_run_skips_empty_script_body() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("with-body", build_writes_marker("with-body")),
            (
                "empty-body",
                json!({
                    "name": "empty-body",
                    "version": "1.0.0",
                    "scripts": { "build": "" },
                }),
            ),
        ],
    );

    pacquet
        .with_arg("-r")
        .with_arg("run")
        .with_arg("--report-summary")
        .with_arg("build")
        .assert()
        .success();

    let statuses = summary_statuses(&workspace);
    assert_eq!(statuses.get("with-body").map(String::as_str), Some("passed"));
    assert_eq!(
        statuses.get("empty-body").map(String::as_str),
        Some("skipped"),
        "empty `build` body should be Skipped, not Passed; got {statuses:?}",
    );

    drop(root);
}

/// `pnpm -r run .hidden` is rejected outside a lifecycle context with
/// `ERR_PNPM_HIDDEN_SCRIPT`. Mirrors pnpm's
/// `throwOrFilterHiddenScripts` call from `runRecursive.ts:113-115`,
/// applied once for the user-typed script name.
#[test]
fn recursive_run_rejects_hidden_script_name() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[(
            "project-1",
            json!({
                "name": "project-1",
                "version": "1.0.0",
                "scripts": { ".secret": "true" },
            }),
        )],
    );

    let output =
        pacquet.with_arg("-r").with_arg("run").with_arg(".secret").output().expect("spawn pacquet");
    assert!(!output.status.success(), "hidden script must fail outside a lifecycle");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_HIDDEN_SCRIPT"),
        "stderr should carry the hidden-script error code, got: {stderr}",
    );

    drop(root);
}

/// When NO workspace project defines the requested hidden `.name`
/// script, pnpm's `runRecursive` short-circuits at the truthy-body
/// gate before reaching `throwOrFilterHiddenScripts`, so the error
/// surfaces as `ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT` rather than
/// `ERR_PNPM_HIDDEN_SCRIPT`. Pins the gate ordering.
#[test]
fn recursive_run_missing_hidden_script_reports_no_script_not_hidden() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &[("project-1", build_writes_marker("project-1"))]);

    let output = pacquet
        .with_arg("-r")
        .with_arg("run")
        .with_arg(".missing")
        .output()
        .expect("spawn pacquet");
    assert!(!output.status.success(), "missing script must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT"),
        "expected the no-script code, got: {stderr}",
    );
    assert!(
        !stderr.contains("ERR_PNPM_HIDDEN_SCRIPT"),
        "must not raise HIDDEN_SCRIPT when no project defines the script: {stderr}",
    );

    drop(root);
}

/// With `enable-pre-post-scripts=true`, `pacquet -r run build` runs
/// `prebuild` and `postbuild` around the main `build` per project,
/// matching pnpm's `runRecursive` which binds `runScript` with
/// `runScriptOptions.enablePrePostScripts` (runRecursive.ts:147,156).
#[test]
fn recursive_run_runs_pre_and_post_when_enabled() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[(
            "project-1",
            json!({
                "name": "project-1",
                "version": "1.0.0",
                "scripts": {
                    "prebuild": "touch pre.txt",
                    "build": "touch ran.txt",
                    "postbuild": "touch post.txt",
                },
            }),
        )],
    );

    pacquet
        .with_env("PNPM_CONFIG_ENABLE_PRE_POST_SCRIPTS", "true")
        .with_arg("-r")
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();

    let pkg = workspace.join("project-1");
    assert!(pkg.join("pre.txt").exists(), "prebuild should have run");
    assert!(pkg.join("ran.txt").exists(), "build should have run");
    assert!(pkg.join("post.txt").exists(), "postbuild should have run");

    drop(root);
}

/// Recursion guard: when `npm_lifecycle_event` matches the requested
/// script AND `PNPM_SCRIPT_SRC_DIR` matches a project root, that
/// project is skipped so a script that itself invokes `pacquet -r run
/// <name>` doesn't recurse without bound. Mirrors pnpm's
/// `runRecursive.ts:108-110`.
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

/// `pacquet -r run` with no script name surfaces pnpm's
/// `ERR_PNPM_SCRIPT_NAME_IS_REQUIRED` typed error variant, matching the
/// `PnpmError('SCRIPT_NAME_IS_REQUIRED', ...)` throw in pnpm's
/// `runRecursive.ts:50-52`.
#[test]
fn recursive_run_without_script_name_errors_with_script_name_is_required() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &[("project-1", build_writes_marker("project-1"))]);

    let output = pacquet.with_arg("-r").with_arg("run").output().expect("spawn pacquet");
    assert!(!output.status.success(), "missing script name in recursive mode must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_SCRIPT_NAME_IS_REQUIRED"),
        "stderr should carry the script-name-required code, got: {stderr}",
    );

    drop(root);
}
