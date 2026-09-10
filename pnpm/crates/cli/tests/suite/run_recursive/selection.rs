use super::{
    Command, CommandExtra, CommandTempCwd, Value, WORKSPACE_ROOT_START_DIRS,
    build_appends_run_order, build_writes_marker, fs, json, process_group_probe,
    workspace_root_run_selection, write_executable, write_workspace,
    write_workspace_with_root_and_packages,
};
use assert_cmd::{assert::OutputAssertExt, cargo::CommandCargoExt};

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
