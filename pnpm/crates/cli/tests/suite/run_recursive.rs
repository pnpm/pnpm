//! Recursive-run integration tests. The build scripts run through
//! pacquet's `sh -c` executor, so the whole file is gated to Unix —
//! same as the single-package `run` tests.
#![cfg(unix)]

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

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

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).expect("write executable");
    let mut perms = fs::metadata(path).expect("stat executable").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).expect("chmod +x");
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

/// A package whose `build` script appends its name to a shared `../order.log`
/// (the workspace root), so a test can read back the order the recursive
/// runner executed the selected projects in.
fn build_appends_run_order(name: &str) -> Value {
    json!({
        "name": name,
        "version": "1.0.0",
        "scripts": { "build": format!("echo {name} >> ../order.log") },
    })
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
printf "%s %s\n" "$child_group" "$parent_group" >> ../process-groups.txt"#
}

/// `pacquet -r run <script>` runs the script in every workspace project,
/// in topological order derived from the workspace dependency graph.
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
fn top_level_fallback_enters_recursive_run() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let commitlint_writes_marker = |name: &str| {
        json!({
            "name": name,
            "version": "1.0.0",
            "scripts": {
                "commitlint": r#"node -e "require('fs').writeFileSync('ran.txt', '')""#,
            },
        })
    };
    write_workspace(
        &workspace,
        &[
            ("project-1", commitlint_writes_marker("project-1")),
            ("project-2", commitlint_writes_marker("project-2")),
        ],
    );

    pacquet.with_arg("-r").with_arg("commitlint").assert().success();

    for name in ["project-1", "project-2"] {
        assert!(
            workspace.join(name).join("ran.txt").exists(),
            "{name} commitlint script should have run through recursive fallback",
        );
    }

    drop(root);
}

#[test]
fn recursive_lifecycle_aliases_use_recursive_run_options() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let lifecycle_scripts = |name: &str| {
        json!({
            "name": name,
            "version": "1.0.0",
            "scripts": {
                "test": "touch test-ran.txt",
                "start": "touch start-ran.txt",
                "stop": "touch stop-ran.txt",
            },
        })
    };
    write_workspace(
        &workspace,
        &[
            ("project-1", lifecycle_scripts("project-1")),
            ("project-2", lifecycle_scripts("project-2")),
        ],
    );

    for (command, marker) in
        [("test", "test-ran.txt"), ("start", "start-ran.txt"), ("stop", "stop-ran.txt")]
    {
        let _ = fs::remove_file(workspace.join("pnpm-exec-summary.json"));
        std::process::Command::cargo_bin("pnpm")
            .expect("find pacquet binary")
            .with_current_dir(&workspace)
            .with_arg("-r")
            .with_arg("--report-summary")
            .with_arg(command)
            .assert()
            .success();

        for name in ["project-1", "project-2"] {
            assert!(workspace.join(name).join(marker).exists(), "{command} should run in {name}");
        }
        let statuses = summary_statuses(&workspace);
        assert_eq!(statuses.get("project-1").map(String::as_str), Some("passed"));
        assert_eq!(statuses.get("project-2").map(String::as_str), Some("passed"));
    }

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

/// Starting inside a member project is what the flag exists for
/// (pnpm/pnpm#13031), so every case is checked from both.
const WORKSPACE_ROOT_START_DIRS: [&str; 2] = [".", "project-1"];

/// The projects whose `build` ran, in workspace order, naming the root
/// project `"<root>"`.
fn workspace_root_run_selection(start_dir: &str, filter: Option<&str>) -> Vec<String> {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("project-1", build_writes_marker("project-1")),
            ("project-2", build_writes_marker("project-2")),
        ],
    );
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

    let mut args = vec!["--dir", start_dir, "-r", "-w"];
    if let Some(filter) = filter {
        args.extend(["--filter", filter]);
    }
    args.extend(["run", "build"]);
    pacquet.with_args(args).assert().success();

    let ran = std::iter::once(("<root>", workspace.join("root-ran.txt")))
        .chain(["project-1", "project-2"].map(|name| (name, workspace.join(name).join("ran.txt"))))
        .filter(|(_, marker)| marker.exists())
        .map(|(name, _)| name.to_string())
        .collect();

    drop(root); // cleanup
    ran
}

/// Write a `packages/*` workspace with a root `package.json` (whose
/// `build` script writes `root-ran.txt`) plus `project-1` / `project-2`
/// sub-packages, so a recursive run has both a root project and non-root
/// projects to choose between.
fn write_workspace_with_root_and_packages(workspace: &Path) {
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
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
    for name in ["project-1", "project-2"] {
        let dir = workspace.join("packages").join(name);
        fs::create_dir_all(&dir).expect("create package dir");
        fs::write(dir.join("package.json"), build_writes_marker(name).to_string())
            .expect("write package.json");
    }
}

/// A `[<since>]` changed-packages selector scopes a recursive `run` to
/// the projects the git diff touches.
#[test]
fn recursive_run_diff_selector_selects_changed_projects() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("project-1", build_writes_marker("project-1")),
            ("project-2", build_writes_marker("project-2")),
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
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();

    assert!(
        workspace.join("project-1").join("ran.txt").exists(),
        "the changed project-1 should run the build script",
    );
    assert!(
        !workspace.join("project-2").join("ran.txt").exists(),
        "the unchanged project-2 must stay outside the selection",
    );

    drop(root);
}

/// A bare-semver range naming a sibling is not a workspace edge under the
/// default `link-workspace-packages: false`, matching pnpm. `app` listing
/// `lib` as a bare `1.0.0` dependency therefore has no edge to it, so
/// `--filter app...` (which follows dependencies) selects only `app`.
#[test]
fn recursive_run_does_not_follow_bare_semver_deps_as_workspace_edges() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut app = build_writes_marker("app");
    app["dependencies"] = json!({ "lib": "1.0.0" });
    write_workspace(&workspace, &[("lib", build_writes_marker("lib")), ("app", app)]);

    pacquet
        .with_arg("-r")
        .with_arg("--filter")
        .with_arg("app...")
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();

    assert!(workspace.join("app").join("ran.txt").exists(), "the selected app should run");
    assert!(
        !workspace.join("lib").join("ran.txt").exists(),
        "a bare-semver range is not a workspace edge under the default link-workspace-packages: false, so app... must not reach lib",
    );

    drop(root);
}

#[test]
fn recursive_run_no_sort_uses_workspace_order() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            (
                "z-app",
                json!({
                    "name": "z-app",
                    "version": "1.0.0",
                    "scripts": { "build": "echo z-app >> ../order.log" },
                    "dependencies": { "a-lib": "workspace:*" },
                }),
            ),
            ("a-lib", build_appends_run_order("a-lib")),
        ],
    );

    pacquet
        .with_arg("--workspace-concurrency=1")
        .with_arg("--no-sort")
        .with_arg("--filter-prod=z-app")
        .with_arg("--filter=a-lib")
        .with_arg("-r")
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();

    let order = fs::read_to_string(workspace.join("order.log")).expect("read order log");
    assert_eq!(order, "z-app\na-lib\n");

    drop(root);
}

#[test]
fn recursive_run_reads_sort_from_workspace_config() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            (
                "app",
                json!({
                    "name": "app",
                    "version": "1.0.0",
                    "scripts": { "build": "echo app >> ../order.log" },
                    "dependencies": { "lib": "workspace:*" },
                }),
            ),
            ("lib", build_appends_run_order("lib")),
        ],
    );
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - app\n  - lib\nsort: false\n")
        .expect("write workspace settings");

    pacquet.with_args(["--workspace-concurrency=1", "-r", "run", "build"]).assert().success();

    let order = fs::read_to_string(workspace.join("order.log")).expect("read order log");
    assert_eq!(order, "app\nlib\n");

    drop(root);
}

#[test]
fn recursive_run_reads_reverse_from_workspace_config() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            (
                "app",
                json!({
                    "name": "app",
                    "version": "1.0.0",
                    "scripts": { "build": "echo app >> ../order.log" },
                    "dependencies": { "lib": "workspace:*" },
                }),
            ),
            ("lib", build_appends_run_order("lib")),
        ],
    );
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - app\n  - lib\nreverse: true\n",
    )
    .expect("write workspace settings");

    pacquet.with_args(["-r", "run", "build"]).assert().success();

    let order = fs::read_to_string(workspace.join("order.log")).expect("read order log");
    assert_eq!(order, "app\nlib\n");

    fs::remove_file(workspace.join("order.log")).expect("clear order log");
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["-r", "--no-reverse", "run", "build"])
        .assert()
        .success();
    let order = fs::read_to_string(workspace.join("order.log")).expect("read order log");
    assert_eq!(order, "lib\napp\n");

    drop(root);
}

fn assert_recursive_run_bail_cancels_in_flight(shell_emulator: bool) {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = |name: &str, body: &str| json!({ "name": name, "version": "1.0.0", "scripts": { "build": body } });
    write_workspace(
        &workspace,
        &[
            (
                "a-slow-1",
                manifest(
                    "a-slow-1",
                    r#"node -e "require('fs').writeFileSync('ran.txt', ''); setTimeout(() => {}, 5000)""#,
                ),
            ),
            (
                "b-fails",
                manifest(
                    "b-fails",
                    r#"node -e "const fs = require('fs'); const wait = () => fs.existsSync('../a-slow-1/ran.txt') && fs.existsSync('../c-slow-2/ran.txt') ? process.exit(1) : setTimeout(wait, 10); wait()""#,
                ),
            ),
            (
                "c-slow-2",
                manifest(
                    "c-slow-2",
                    r#"node -e "require('fs').writeFileSync('ran.txt', ''); setTimeout(() => {}, 5000)""#,
                ),
            ),
            ("z-queued", manifest("z-queued", "touch ran.txt")),
        ],
    );
    if shell_emulator {
        fs::write(
            workspace.join("pnpm-workspace.yaml"),
            "packages:\n  - a-slow-1\n  - b-fails\n  - c-slow-2\n  - z-queued\nshellEmulator: true\n",
        )
        .expect("enable the shell emulator");
    }

    let start = Instant::now();
    let output = pacquet
        .with_args([
            "--workspace-concurrency=3",
            "--no-sort",
            "--report-summary",
            "-r",
            "run",
            "build",
        ])
        .output()
        .expect("spawn pacquet");
    let elapsed = start.elapsed();
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(!output.status.success(), "the failing project should fail the run");
    eprintln!("recursive run elapsed: {elapsed:?}");
    assert!(
        elapsed < Duration::from_secs(4),
        "bail should interrupt the five-second in-flight scripts",
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

/// The top-level `--if-present` spelling with a shorthand script — the
/// shape the repo's own `test-pkgs-branch` script uses
/// (`pnpm --workspace-concurrency=1 --no-sort --if-present <script>`) —
/// is the same clean no-op when no package defines the script.
#[test]
fn recursive_top_level_if_present_is_a_noop_when_no_package_has_the_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &[("project-1", build_writes_marker("project-1"))]);

    pacquet
        .with_arg("--workspace-concurrency=1")
        .with_arg("--no-sort")
        .with_arg("--if-present")
        .with_arg("-r")
        .with_arg("lint")
        .assert()
        .success();

    drop(root);
}

/// `pnpm -r run <name>` skips a project whose `<name>` script body is
/// the empty string. An empty script body is falsy in JS and so is
/// skipped; pacquet checks for it explicitly because `manifest.script`
/// returns `Some("")`.
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
/// `ERR_PNPM_HIDDEN_SCRIPT`, applied once for the user-typed script name.
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
/// script, the truthy-body gate short-circuits before the hidden-script
/// check runs, so the error surfaces as
/// `ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT` rather than
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
/// `prebuild` and `postbuild` around the main `build` per project.
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

/// A `/pattern/` selector can match several scripts in one project, but
/// the summary carries a single status per project and the exit code is
/// derived from it. Under `--no-bail` a later script's success must not
/// erase an earlier one's failure.
#[test]
fn recursive_run_keeps_a_failure_when_a_later_selected_script_passes() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[(
            "pkg",
            json!({
                "name": "pkg",
                "version": "1.0.0",
                "scripts": {
                    // Alphabetical order puts the failure first, so a
                    // regression reports the project as passed.
                    "check:a": "exit 1",
                    "check:b": "true",
                },
            }),
        )],
    );

    pacquet
        .with_args(["-r", "run", "--no-bail", "--report-summary", "/^check:/"])
        .assert()
        .failure();

    let statuses = summary_statuses(&workspace);
    assert_eq!(
        statuses.get("pkg").map(String::as_str),
        Some("failure"),
        "a failed script must survive a later passing one: {statuses:?}",
    );

    drop(root);
}

#[test]
fn recursive_run_keeps_a_pass_when_a_later_matching_script_is_empty() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[(
            "pkg",
            json!({
                "name": "pkg",
                "version": "1.0.0",
                "scripts": {
                    "check:a": "true",
                    "check:b": "",
                },
            }),
        )],
    );

    pacquet.with_args(["-r", "run", "--report-summary", "/^check:/"]).assert().success();

    let statuses = summary_statuses(&workspace);
    assert_eq!(
        statuses.get("pkg").map(String::as_str),
        Some("passed"),
        "a no-op script must not erase the passing one: {statuses:?}",
    );

    drop(root);
}

/// A package whose `test` script echoes a fixed marker, so a test can
/// assert on the reporter's framing of it rather than on the payload.
fn echoes_ok(name: &str) -> Value {
    json!({
        "name": name,
        "version": "1.0.0",
        "scripts": { "test": "echo OK" },
    })
}

/// Sorted so a test does not depend on the order two concurrent projects
/// finish in.
fn sorted_lines(stdout: &[u8]) -> Vec<String> {
    let stdout = String::from_utf8_lossy(stdout);
    eprintln!("STDOUT:\n{stdout}\n");
    let mut lines = stdout.trim().lines().map(str::to_string).collect::<Vec<_>>();
    lines.sort();
    lines
}

mod selection;

mod task_graph;

mod recovery;

mod output;

mod environment;
