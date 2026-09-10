use super::{
    CommandExtra, CommandTempCwd, PermissionsExt, fs, json, process_group_probe,
    write_concurrency_probe, write_executable, write_workspace,
};
use assert_cmd::assert::OutputAssertExt;

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
