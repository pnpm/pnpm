#[cfg(unix)]
use super::process_group_probe;
use super::{
    CONCURRENCY_PROBE_COMMAND, CommandExtra, CommandTempCwd, fs, json, write_concurrency_probe,
    write_node_bin, write_workspace,
};
#[cfg(unix)]
use crate::_utils::terminal::Terminal;
use assert_cmd::assert::OutputAssertExt;

/// A per-task `concurrency: 1` serializes the scripts just as firmly as a
/// dependency chain does, so at a terminal they must stay in pacquet's
/// process group too — the scheduler never has two of them in flight to
/// keep apart.
///
/// Unix-only by subject: the assertion compares POSIX process groups,
/// which Windows has no counterpart for — pnpm keeps a script's children
/// in a job object there instead.
#[cfg(unix)]
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

    let terminal = Terminal::open();
    let mut process = terminal.spawn_foreground(pacquet.with_args(["-r", "run", "build"]));
    let status = process.wait().expect("wait for pacquet");
    assert!(status.success(), "pacquet should succeed on the terminal");

    let groups =
        fs::read_to_string(workspace.join("process-groups.txt")).expect("read process groups");
    let mut lines = groups.lines();
    let parent_group = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .expect("parent process group")
        .to_string();
    for line in groups.lines() {
        let child_group = line
            .split_whitespace()
            .next()
            .expect("child process group");
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
            "scripts": { "build": CONCURRENCY_PROBE_COMMAND },
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

    pacquet
        .with_args(["--workspace-concurrency=2", "-r", "run", "build"])
        .assert()
        .success();

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
            "scripts": { "build": "node ../track-task-concurrency.cjs" },
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
    // `mkdirSync` is the lock: it throws rather than succeeding twice, so
    // a second task running at the same time records that it overlapped.
    fs::write(
        workspace.join("track-task-concurrency.cjs"),
        r"const fs = require('fs')
let ownsLock = false
try {
  fs.mkdirSync('../build-active')
  ownsLock = true
} catch {
  fs.writeFileSync('../exceeded-task-concurrency', '')
}
setTimeout(() => {
  fs.writeFileSync('ran.txt', '')
  if (ownsLock) fs.rmdirSync('../build-active')
}, 200)
",
    )
    .expect("write task concurrency probe");

    pacquet
        .with_args(["--workspace-concurrency=3", "-r", "run", "build"])
        .assert()
        .success();

    assert!(
        !workspace.join("exceeded-task-concurrency").exists(),
        "only one build task should run at a time",
    );
    for name in ["project-1", "project-2", "project-3"] {
        assert!(
            workspace
                .join(name)
                .join("ran.txt")
                .exists(),
            "{name} should have run",
        );
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

    pacquet
        .with_args(["-r", "run", "build"])
        .assert()
        .success();

    for name in ["project-1", "project-2"] {
        assert!(
            workspace
                .join(name)
                .join("pnp-loader-ran.txt")
                .exists(),
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
                // Announces itself, then waits for its peer to do the
                // same. Only overlapping runs see each other's marker, so
                // a serialized pair times out and fails.
                "build": format!(
                    r#"node -e "const fs = require('fs'); fs.writeFileSync('../{name}.started', ''); const started = Date.now(); (function poll () {{ if (fs.existsSync('../{peer}.started')) process.exit(0); if (Date.now() - started > 30000) process.exit(1); setTimeout(poll, 10) }})()""#
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
        let bin_dir = workspace
            .join(name)
            .join("node_modules")
            .join(".bin");
        fs::create_dir_all(&bin_dir).expect("create node_modules/.bin");
        write_node_bin(&bin_dir, "commitlint", "require('fs').writeFileSync('bin-ran.txt', '')\n");
    }

    let output = pacquet
        .with_arg("-r")
        .with_arg("commitlint")
        .output()
        .expect("spawn pacquet");
    assert!(!output.status.success(), "recursive shorthand without matching scripts must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT"),
        "recursive shorthand must report the recursive no-script error, got: {stderr}",
    );
    for name in ["project-1", "project-2"] {
        assert!(
            !workspace
                .join(name)
                .join("bin-ran.txt")
                .exists(),
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
    write_node_bin(&bin_dir, "say-hi", "require('fs').writeFileSync('hi.txt', '')\n");

    pacquet
        .with_arg("-r")
        .with_arg("run")
        .with_arg("build")
        .assert()
        .success();
    assert!(
        pkg_root.join("hi.txt").exists(),
        "recursive run should resolve `say-hi` from the package's node_modules/.bin",
    );

    drop(root);
}
