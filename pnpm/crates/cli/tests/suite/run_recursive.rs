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

/// A single filtered script cannot run alongside a sibling, so it must
/// stay in pacquet's own process group: a child moved into its own group
/// is stopped the moment it reads from the terminal.
#[test]
fn filtered_run_keeps_single_script_in_foreground_process_group() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            (
                "project-1",
                json!({
                    "name": "project-1",
                    "version": "1.0.0",
                    "scripts": { "prompt": process_group_probe() },
                }),
            ),
            ("project-2", build_writes_marker("project-2")),
        ],
    );

    pacquet.with_args(["--filter", "project-1", "run", "prompt"]).assert().success();

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

/// A per-task `concurrency: 1` serializes the scripts just as firmly as a
/// dependency chain does, so they must stay in pacquet's process group
/// too — the scheduler never has two of them in flight to keep apart.
#[test]
fn task_concurrency_of_one_keeps_scripts_in_the_foreground_process_group() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = |name: &str| {
        json!({
            "name": name,
            "version": "1.0.0",
            "scripts": { "build": process_group_probe() },
        })
    };
    write_workspace(
        &workspace,
        &[
            ("project-1", manifest("project-1")),
            ("project-2", manifest("project-2")),
            ("project-3", manifest("project-3")),
        ],
    );
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        concat!(
            "packages:\n",
            "  - project-1\n",
            "  - project-2\n",
            "  - project-3\n",
            "tasks:\n",
            "  build:\n",
            "    concurrency: 1\n",
        ),
    )
    .expect("write task settings");

    pacquet.with_args(["-r", "run", "build"]).assert().success();

    let groups =
        fs::read_to_string(workspace.join("process-groups.txt")).expect("read process groups");
    let mut lines = groups.lines();
    let parent_group = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .expect("parent process group")
        .to_string();
    for line in groups.lines() {
        let child_group = line.split_whitespace().next().expect("child process group");
        assert_eq!(
            child_group, parent_group,
            "every serialized script must share pacquet's process group",
        );
    }
    assert_eq!(groups.lines().count(), 3, "every project should have run");

    drop(root);
}

#[test]
fn recursive_run_respects_workspace_concurrency() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = |name: &str| {
        json!({
            "name": name,
            "version": "1.0.0",
            "scripts": { "build": "sh ../track-concurrency.sh" },
        })
    };
    write_workspace(
        &workspace,
        &[
            ("project-1", manifest("project-1")),
            ("project-2", manifest("project-2")),
            ("project-3", manifest("project-3")),
        ],
    );
    write_concurrency_probe(&workspace);

    pacquet.with_args(["--workspace-concurrency=2", "-r", "run", "build"]).assert().success();

    assert!(workspace.join("saw-parallel").exists(), "two scripts should overlap");
    assert!(
        !workspace.join("exceeded-concurrency").exists(),
        "no more than two scripts should overlap",
    );

    drop(root);
}

#[test]
fn recursive_run_respects_task_concurrency() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = |name: &str| {
        json!({
            "name": name,
            "version": "1.0.0",
            "scripts": { "build": "sh ../track-task-concurrency.sh" },
        })
    };
    write_workspace(
        &workspace,
        &[
            ("project-1", manifest("project-1")),
            ("project-2", manifest("project-2")),
            ("project-3", manifest("project-3")),
        ],
    );
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        concat!(
            "packages:\n",
            "  - project-1\n",
            "  - project-2\n",
            "  - project-3\n",
            "tasks:\n",
            "  build:\n",
            "    concurrency: 1\n",
        ),
    )
    .expect("write task settings");
    write_executable(
        &workspace.join("track-task-concurrency.sh"),
        r#"if mkdir ../build-active 2>/dev/null; then
  owns_lock=1
else
  touch ../exceeded-task-concurrency
fi
sleep 0.2
touch ran.txt
[ "$owns_lock" = 1 ] && rmdir ../build-active
"#,
    );

    pacquet.with_args(["--workspace-concurrency=3", "-r", "run", "build"]).assert().success();

    assert!(
        !workspace.join("exceeded-task-concurrency").exists(),
        "only one build task should run at a time",
    );
    for name in ["project-1", "project-2", "project-3"] {
        assert!(workspace.join(name).join("ran.txt").exists(), "{name} should have run");
    }

    drop(root);
}

/// Without a workspace-level loader, recursive run preloads the `.pnp.cjs`
/// belonging to each selected project rather than resolving once from the
/// invocation directory.
#[test]
fn recursive_run_preloads_each_project_pnp_loader() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = |name: &str| {
        json!({
            "name": name,
            "version": "1.0.0",
            "scripts": { "build": "node -e 0" },
        })
    };
    write_workspace(
        &workspace,
        &[("project-1", manifest("project-1")), ("project-2", manifest("project-2"))],
    );
    for name in ["project-1", "project-2"] {
        fs::write(
            workspace.join(name).join(".pnp.cjs"),
            "require('fs').writeFileSync('pnp-loader-ran.txt', '')",
        )
        .expect("write project PnP loader");
    }

    pacquet.with_args(["-r", "run", "build"]).assert().success();

    for name in ["project-1", "project-2"] {
        assert!(
            workspace.join(name).join("pnp-loader-ran.txt").exists(),
            "{name} should preload its own PnP loader",
        );
    }

    drop(root);
}

#[test]
fn parallel_before_run_starts_selected_projects_concurrently() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let waits_for_peer = |name: &str, peer: &str| {
        json!({
            "name": name,
            "version": "1.0.0",
            "scripts": {
                "build": format!(
                    "touch ../{name}.started; \
                     attempts=0; \
                     while [ ! -f ../{peer}.started ] && [ \"$attempts\" -lt 100 ]; do \
                       sleep 0.01; attempts=$((attempts + 1)); \
                     done; \
                     test -f ../{peer}.started"
                ),
            },
        })
    };
    write_workspace(
        &workspace,
        &[
            ("project-1", waits_for_peer("project-1", "project-2")),
            ("project-2", waits_for_peer("project-2", "project-1")),
        ],
    );

    pacquet
        .with_arg("-r")
        .with_arg("--filter=./project-*")
        .with_arg("--parallel")
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();

    assert!(
        workspace.join("project-1.started").exists(),
        "project-1 should start while project-2 is waiting",
    );
    assert!(
        workspace.join("project-2.started").exists(),
        "project-2 should start while project-1 is waiting",
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

/// A member's script resolves binaries from the workspace root's
/// `node_modules/.bin` — pnpm puts it on PATH via `extraBinPaths`, so
/// root-level dev tools are callable from every workspace project.
#[test]
fn recursive_run_finds_workspace_root_bin_on_path() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[(
            "project-1",
            json!({
                "name": "project-1",
                "version": "1.0.0",
                "scripts": { "build": "root-tool" },
            }),
        )],
    );
    let bin_dir = workspace.join("node_modules").join(".bin");
    fs::create_dir_all(&bin_dir).expect("create workspace-root node_modules/.bin");
    write_executable(&bin_dir.join("root-tool"), "#!/bin/sh\ntouch root-tool-ran.txt\n");

    pacquet.with_arg("-r").with_arg("run").with_arg("build").assert().success();

    assert!(
        workspace.join("project-1").join("root-tool-ran.txt").exists(),
        "the workspace root's node_modules/.bin should be on the script's PATH",
    );

    drop(root);
}

/// The project's own `node_modules/.bin` outranks the workspace root's:
/// when both provide the same tool, the member's copy runs. Ports the
/// `testBinPriority` step of `pnpm recursive run finds bins from the root
/// of the workspace` (`pnpm/test/recursive/run.ts`).
#[test]
fn recursive_run_prefers_project_bin_over_workspace_root_bin() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[(
            "project-1",
            json!({
                "name": "project-1",
                "version": "1.0.0",
                "scripts": { "build": "print-version > version.txt" },
            }),
        )],
    );
    for (dir, version) in [(workspace.clone(), "2.0.0"), (workspace.join("project-1"), "1.0.0")] {
        let bin_dir = dir.join("node_modules").join(".bin");
        fs::create_dir_all(&bin_dir).expect("create node_modules/.bin");
        write_executable(&bin_dir.join("print-version"), &format!("#!/bin/sh\necho {version}\n"));
    }

    pacquet.with_arg("-r").with_arg("run").with_arg("build").assert().success();

    let version = fs::read_to_string(workspace.join("project-1").join("version.txt"))
        .expect("read version.txt");
    assert_eq!(version.trim(), "1.0.0", "the project's own bin must win over the root's");

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
fn top_level_fallback_does_not_exec_local_bin_recursively() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("project-1", json!({ "name": "project-1", "version": "1.0.0", "scripts": {} })),
            ("project-2", json!({ "name": "project-2", "version": "1.0.0", "scripts": {} })),
        ],
    );
    for name in ["project-1", "project-2"] {
        let bin_dir = workspace.join(name).join("node_modules").join(".bin");
        fs::create_dir_all(&bin_dir).expect("create node_modules/.bin");
        write_executable(&bin_dir.join("commitlint"), "#!/bin/sh\ntouch bin-ran.txt\n");
    }

    let output = pacquet.with_arg("-r").with_arg("commitlint").output().expect("spawn pacquet");
    assert!(!output.status.success(), "recursive shorthand without matching scripts must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT"),
        "recursive shorthand must report the recursive no-script error, got: {stderr}",
    );
    for name in ["project-1", "project-2"] {
        assert!(
            !workspace.join(name).join("bin-ran.txt").exists(),
            "{name} local binary must not run from recursive shorthand",
        );
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

#[test]
fn recursive_run_workspace_root_selects_only_the_root_project() {
    for start_dir in WORKSPACE_ROOT_START_DIRS {
        assert_eq!(
            workspace_root_run_selection(start_dir, None),
            ["<root>"],
            "--dir {start_dir}: --workspace-root selects the root project alone",
        );
    }
}

/// pnpm reports `Scope: 2 of 3 workspace projects` for this command.
#[test]
fn recursive_run_workspace_root_adds_the_root_to_a_filter_selection() {
    for start_dir in WORKSPACE_ROOT_START_DIRS {
        assert_eq!(
            workspace_root_run_selection(start_dir, Some("project-1")),
            ["<root>", "project-1"],
            "--dir {start_dir}: --workspace-root keeps the --filter-selected project",
        );
    }
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

/// `pacquet -r --filter <name> run <script>` runs the script only in the
/// `--filter`-selected project, leaving the rest untouched. Threads
/// `config.filter` through the recursive dispatch to build the selected
/// projects graph.
#[test]
fn recursive_run_filter_selects_only_matching_project() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("project-1", build_writes_marker("project-1")),
            ("project-2", build_writes_marker("project-2")),
            ("project-3", build_writes_marker("project-3")),
        ],
    );

    pacquet
        .with_arg("-r")
        .with_arg("--filter")
        .with_arg("project-1")
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();

    assert!(
        workspace.join("project-1").join("ran.txt").exists(),
        "the selected project-1 should run",
    );
    for name in ["project-2", "project-3"] {
        assert!(
            !workspace.join(name).join("ran.txt").exists(),
            "{name} is not selected by --filter and must not run",
        );
    }

    drop(root);
}

/// An exclude selector (`!<name>`) runs the script in every project
/// except the excluded one — the shape pnpm's release workflow leans on
/// with `--filter=!pnpm`.
#[test]
fn recursive_run_exclude_filter_skips_excluded_project() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("project-1", build_writes_marker("project-1")),
            ("project-2", build_writes_marker("project-2")),
            ("project-3", build_writes_marker("project-3")),
        ],
    );

    pacquet
        .with_arg("-r")
        .with_arg("--filter")
        .with_arg("!project-2")
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();

    assert!(workspace.join("project-1").join("ran.txt").exists(), "project-1 should run");
    assert!(workspace.join("project-3").join("ran.txt").exists(), "project-3 should run");
    assert!(
        !workspace.join("project-2").join("ran.txt").exists(),
        "project-2 is excluded by !project-2 and must not run",
    );

    drop(root);
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

/// A bare `--filter` (no `-r`) enters recursive mode CLI-wide: the script
/// runs only in the selected project even though `-r` was never passed.
#[test]
fn filter_without_recursive_flag_enters_recursive_run() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("project-1", build_writes_marker("project-1")),
            ("project-2", build_writes_marker("project-2")),
        ],
    );

    pacquet
        .with_arg("--filter")
        .with_arg("project-1")
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();

    assert!(
        workspace.join("project-1").join("ran.txt").exists(),
        "the selected project-1 should run",
    );
    assert!(
        !workspace.join("project-2").join("ran.txt").exists(),
        "a bare --filter (no -r) should still scope the run to the selection",
    );

    drop(root);
}

#[test]
fn filtered_run_prints_the_script_command_unless_silent() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("project-1", build_writes_marker("project-1")),
            ("project-2", build_writes_marker("project-2")),
        ],
    );

    let output = pacquet
        .with_arg("--filter")
        .with_arg("project-1")
        .with_arg("run")
        .with_arg("build")
        .output()
        .expect("run filtered build");
    assert!(output.status.success(), "filtered build failed: {output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("$ touch ran.txt"),
        "filtered build must print its script command: {output:?}",
    );

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_arg("--silent")
        .with_arg("--filter")
        .with_arg("project-2")
        .with_arg("run")
        .with_arg("build")
        .output()
        .expect("run silent filtered build");
    assert!(output.status.success(), "silent filtered build failed: {output:?}");
    assert!(
        workspace.join("project-2").join("ran.txt").is_file(),
        "silent filtered build must still execute its script: {output:?}",
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("$ touch ran.txt"),
        "silent filtered build must omit its script command: {output:?}",
    );

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_arg("--reporter=ndjson")
        .with_arg("--filter")
        .with_arg("project-1")
        .with_arg("run")
        .with_arg("build")
        .output()
        .expect("run filtered build with the NDJSON reporter");
    assert!(output.status.success(), "NDJSON filtered build failed: {output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty(), "NDJSON filtered build must emit reporter records");
    assert!(
        stderr.lines().all(|line| serde_json::from_str::<Value>(line).is_ok()),
        "NDJSON filtered build must contain only JSON records: {stderr}",
    );

    drop(root);
}

/// In a workspace with both a root project and sub-packages, a default
/// recursive `run` (no inclusion filter) auto-excludes the workspace
/// root via the `!{<workspace-root>}` augmentation. The sub-packages
/// run; the root does not.
#[test]
fn recursive_run_auto_excludes_workspace_root() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace_with_root_and_packages(&workspace);

    pacquet.with_arg("-r").with_arg("run").with_arg("build").assert().success();

    assert!(workspace.join("packages/project-1/ran.txt").exists(), "project-1 should run");
    assert!(workspace.join("packages/project-2/ran.txt").exists(), "project-2 should run");
    assert!(
        !workspace.join("root-ran.txt").exists(),
        "the workspace root must be auto-excluded from a default recursive run",
    );

    drop(root);
}

/// `--include-workspace-root` keeps the root in the selection the
/// previous test drops it from, so its `build` runs alongside the
/// sub-packages'.
#[test]
fn include_workspace_root_flag_keeps_the_root_in_a_recursive_run() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace_with_root_and_packages(&workspace);

    pacquet
        .with_arg("-r")
        .with_arg("--include-workspace-root")
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();

    assert!(workspace.join("root-ran.txt").exists(), "the root must run under the flag");
    assert!(workspace.join("packages/project-1/ran.txt").exists(), "project-1 should run");
    assert!(workspace.join("packages/project-2/ran.txt").exists(), "project-2 should run");

    drop(root);
}

/// The flag is the CLI half of the `includeWorkspaceRoot` setting, which
/// reads from `pnpm-workspace.yaml` too — and `--no-include-workspace-root`
/// overrides the setting back off, the way pnpm's `--no-` negation does.
#[test]
fn include_workspace_root_setting_is_read_from_the_workspace_manifest() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace_with_root_and_packages(&workspace);
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nincludeWorkspaceRoot: true\n",
    )
    .expect("write workspace manifest");

    let markers = [
        workspace.join("root-ran.txt"),
        workspace.join("packages/project-1/ran.txt"),
        workspace.join("packages/project-2/ran.txt"),
    ];

    pacquet.with_arg("-r").with_arg("run").with_arg("build").assert().success();
    for marker in &markers {
        assert!(marker.exists(), "{} should run under the setting", marker.display());
        fs::remove_file(marker).expect("clear the marker");
    }

    let mut negated = Command::cargo_bin("pnpm").unwrap();
    negated.current_dir(&workspace);
    negated.args(["-r", "--no-include-workspace-root", "run", "build"]).assert().success();
    assert!(!markers[0].exists(), "--no-include-workspace-root must override the setting");
    // The negation drops the root, not the selection: a run that
    // selected nothing would leave these missing too.
    for marker in &markers[1..] {
        assert!(marker.exists(), "{} should still run", marker.display());
    }

    drop(root);
}

/// An all-exclusion selection (`--filter=!<name>`) also drops the
/// workspace root, matching the release-workflow shape
/// (`--filter=!pnpm --filter=!@pnpm/exe`): `-r --filter=!project-2 run
/// build` runs project-1 only — project-2 is excluded by the selector
/// and the root by the `!{<workspace-root>}` augmentation.
#[test]
fn recursive_run_all_exclusion_filter_also_drops_root() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace_with_root_and_packages(&workspace);

    pacquet
        .with_arg("-r")
        .with_arg("--filter")
        .with_arg("!project-2")
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();

    assert!(workspace.join("packages/project-1/ran.txt").exists(), "project-1 should run");
    assert!(
        !workspace.join("packages/project-2/ran.txt").exists(),
        "project-2 is excluded by the !project-2 selector",
    );
    assert!(
        !workspace.join("root-ran.txt").exists(),
        "an all-exclusion selection must also drop the workspace root",
    );

    drop(root);
}

/// The root auto-exclusion is built relative to `--dir`, so it still
/// fires when the recursive run is launched from a workspace
/// subdirectory: with `--dir packages/project-1`, the `!{<workspace-root>}`
/// selector resolves through a non-trivial relative path (`../..`) rather
/// than the bare `.`, and the root is still dropped while every non-root
/// package runs.
#[test]
fn recursive_run_from_subdirectory_still_excludes_root() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace_with_root_and_packages(&workspace);

    pacquet
        .with_arg("--dir")
        .with_arg("packages/project-1")
        .with_arg("-r")
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();

    assert!(workspace.join("packages/project-1/ran.txt").exists(), "project-1 should run");
    assert!(workspace.join("packages/project-2/ran.txt").exists(), "project-2 should run");
    assert!(
        !workspace.join("root-ran.txt").exists(),
        "the workspace root must stay excluded even when run from a subdirectory",
    );

    drop(root);
}

/// An all-exclusion `--filter-prod` also drops the workspace root. The
/// root exclusion inherits `follow_prod_deps_only` from the presence of
/// `--filter-prod`, so it lands in the same production-only selection
/// pass as the user's `!project-2`. Both passes are unioned, so if the
/// exclusion landed in the wrong pass the root (and `project-2`) would be
/// re-added; this pins them to the same pass.
#[test]
fn recursive_run_filter_prod_all_exclusion_also_drops_root() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace_with_root_and_packages(&workspace);

    pacquet
        .with_arg("-r")
        .with_arg("--filter-prod")
        .with_arg("!project-2")
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();

    assert!(workspace.join("packages/project-1/ran.txt").exists(), "project-1 should run");
    assert!(
        !workspace.join("packages/project-2/ran.txt").exists(),
        "project-2 is excluded by the !project-2 production selector",
    );
    assert!(
        !workspace.join("root-ran.txt").exists(),
        "the root exclusion must share the production-only pass, so the root is dropped too",
    );

    drop(root);
}

/// When `--filter` narrows the set and no *selected* package defines the
/// script, the error keeps the `ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT` code
/// but switches to the "None of the selected packages" wording (vs. "None
/// of the packages" when every project is selected).
#[test]
fn recursive_run_filter_no_matching_script_reports_no_selected_packages() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[
            ("project-1", build_writes_marker("project-1")),
            ("project-2", json!({ "name": "project-2", "version": "1.0.0" })),
        ],
    );

    let output = pacquet
        .with_arg("-r")
        .with_arg("--filter")
        .with_arg("project-2")
        .with_arg("run")
        .with_arg("build")
        .output()
        .expect("spawn pacquet");
    assert!(!output.status.success(), "a selected package without the script must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT"),
        "stderr should carry the no-script code, got: {stderr}",
    );
    assert!(
        stderr.contains("None of the selected packages"),
        "stderr should use the selected-packages wording, got: {stderr}",
    );

    drop(root);
}

/// `--filter-prod <pkg>...` walks production dependencies only, so a
/// dev-only edge is excluded from the selected set. With `app` depending
/// on `lib` through `devDependencies`, `--filter-prod app...` runs `app`
/// but skips `lib` — whereas plain `--filter app...` would run both.
/// This is what distinguishes `--filter-prod` from `--filter`: the
/// `follow_prod_deps_only` branch builds the graph with dev edges
/// dropped, so the `...` dependency walk never reaches `lib`.
#[test]
fn recursive_run_filter_prod_follows_production_deps_only() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut app = build_writes_marker("app");
    app["devDependencies"] = json!({ "lib": "workspace:*" });
    write_workspace(&workspace, &[("lib", build_writes_marker("lib")), ("app", app)]);

    pacquet
        .with_arg("-r")
        .with_arg("--filter-prod")
        .with_arg("app...")
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();

    assert!(
        workspace.join("app").join("ran.txt").exists(),
        "the --filter-prod-selected app should run",
    );
    assert!(
        !workspace.join("lib").join("ran.txt").exists(),
        "lib is only a dev dependency of app, so --filter-prod's production-only walk must skip it",
    );

    drop(root);
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

/// A mixed `--filter` / `--filter-prod` selection lists prod-selected
/// projects before regular ones. With `alpha` and `beta` independent — so
/// they share one topological chunk — `--filter alpha` `--filter-prod beta`
/// runs `beta` before `alpha`.
#[test]
fn recursive_run_mixed_filter_runs_prod_selected_before_regular() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[("alpha", build_appends_run_order("alpha")), ("beta", build_appends_run_order("beta"))],
    );

    pacquet
        .with_arg("--workspace-concurrency=1")
        .with_arg("-r")
        .with_arg("--filter")
        .with_arg("alpha")
        .with_arg("--filter-prod")
        .with_arg("beta")
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();

    let log = fs::read_to_string(workspace.join("order.log")).expect("read order log");
    assert_eq!(
        log.lines().collect::<Vec<_>>(),
        vec!["beta", "alpha"],
        "prod-selected projects run before regular-selected ones in a mixed selection",
    );

    drop(root);
}

/// A `--filter` that matches no project is a no-op: the run exits 0
/// without raising the no-selected-packages error, since the selected
/// projects graph is empty.
#[test]
fn recursive_run_filter_no_match_is_a_noop() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &[("project-1", build_writes_marker("project-1"))]);

    pacquet
        .with_arg("-r")
        .with_arg("--filter")
        .with_arg("does-not-exist")
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();

    assert!(
        !workspace.join("project-1").join("ran.txt").exists(),
        "no project is selected, so nothing should run",
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

/// Recursive `run` must resolve each package's `node_modules/.bin` on
/// PATH so locally-installed bins (e.g. `tsc`, `eslint`) work, for every
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

#[test]
fn filtered_run_without_script_name_lists_selected_and_root_scripts() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "workspace-root",
            "version": "1.0.0",
            "scripts": { "root-build": "echo root" },
        })
        .to_string(),
    )
    .expect("write root package.json");
    write_workspace(
        &workspace,
        &[
            (
                "project-1",
                json!({
                    "name": "project-1",
                    "version": "1.0.0",
                    "scripts": {
                        "build": "echo project",
                        "test": "echo tested",
                    },
                }),
            ),
            ("project-2", build_writes_marker("project-2")),
        ],
    );

    let output = pacquet
        .with_arg("--filter")
        .with_arg("project-1")
        .with_arg("run")
        .output()
        .expect("spawn pacquet");
    assert!(output.status.success(), "filtered script listing must succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("STDOUT:\n{stdout}\n");
    assert!(stdout.contains("Lifecycle scripts:\n  test\n    echo tested"));
    assert!(stdout.contains("Commands available via \"pnpm run\":\n  build\n    echo project"));
    assert!(stdout.contains(
        "Commands of the root workspace project (to run them, use \"pnpm -w run\"):\n  root-build\n    echo root",
    ));
    assert!(!stdout.contains("touch ran.txt"), "unselected project scripts must not be listed");

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

#[test]
fn recursive_run_filters_hidden_regexp_matches_when_a_visible_script_matches() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(
        &workspace,
        &[(
            "project",
            json!({
                "name": "project",
                "version": "1.0.0",
                "scripts": {
                    "build:visible": "touch visible.txt",
                    ".build:hidden": "touch hidden.txt",
                },
            }),
        )],
    );

    pacquet.with_args(["-r", "run", "/build/"]).assert().success();

    assert!(workspace.join("project").join("visible.txt").exists());
    assert!(!workspace.join("project").join("hidden.txt").exists());

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
