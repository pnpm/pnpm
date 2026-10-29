//! Recursive and parallel runs. A task that ends on its own follows the
//! single-command rule, so its leftovers survive whether it succeeded or
//! failed. A bail stopping the tasks still running is the second place pnpm
//! differs on purpose.

use super::{Leftover, launcher_args, surviving, write_fixture};
use crate::_utils::wait_for_child;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Stdio};

const LEFTOVERS: [Leftover; 2] = [Leftover::Detached, Leftover::Attached];

/// Write a workspace of `projects`, each with a `build` script.
fn write_workspace(workspace: &Path, projects: &[(&str, &str)]) {
    let packages: Vec<String> = projects
        .iter()
        .map(|(name, _)| format!("  - {name}"))
        .collect();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("packages:\n{}\n", packages.join("\n")),
    )
    .expect("write pnpm-workspace.yaml");
    for &(name, build) in projects {
        let dir = workspace.join(name);
        fs::create_dir_all(&dir).expect("create the project");
        let manifest: Value =
            json!({ "name": name, "version": "1.0.0", "scripts": { "build": build } });
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }
}

/// The `build` script of a project that starts [`LEFTOVERS`] through the
/// workspace's launcher and then exits with `code`.
fn launching(code: i32) -> String {
    format!("node ../launcher.js {}", launcher_args(&code.to_string(), &LEFTOVERS).join(" "))
}

/// A script that finishes during a recursive run leaves its leftovers
/// running, as it does when run alone. `turbo run` would end them; pnpm does
/// not.
#[test]
fn leftovers_of_a_finished_script_survive_a_recursive_run() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_fixture(&workspace);
    write_workspace(&workspace, &[("a-launches", &launching(0)), ("b-succeeds", "node -e \"\"")]);

    let mut run = pacquet
        .with_args(["--workspace-concurrency=1", "--no-sort", "-r", "run", "build"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn pacquet");
    let status = wait_for_child(&mut run);
    assert!(status.success(), "every script succeeds");
    assert_eq!(
        surviving(&workspace.join("a-launches"), &LEFTOVERS),
        [true, true],
        "the finished script's leftovers outlive the run",
    );

    drop(root);
}

/// The script whose failure makes the run bail has ended on its own, so the
/// bail stops only the scripts still running, and the failed one's leftovers
/// survive.
#[test]
fn leftovers_of_the_script_a_bail_follows_survive_it() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_fixture(&workspace);
    let running =
        r#"node -e "require('fs').writeFileSync('ran.txt', ''); setTimeout(() => {}, 60000)""#;
    let fails = format!(
        r#"node -e "const fs = require('fs'); const wait = () => fs.existsSync('../b-running/ran.txt') || setTimeout(wait, 10); wait()" && {}"#,
        launching(1),
    );
    write_workspace(&workspace, &[("a-fails", &fails), ("b-running", running)]);

    let mut run = pacquet
        .with_args(["--workspace-concurrency=2", "--no-sort", "-r", "run", "build"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn pacquet");
    let status = wait_for_child(&mut run);
    assert!(!status.success(), "the failing script fails the run");
    assert_eq!(
        surviving(&workspace.join("a-fails"), &LEFTOVERS),
        [true, true],
        "the failed script's leftovers outlive the bail",
    );

    drop(root);
}

/// A bail asks the scripts still running to stop with `SIGTERM`, so one
/// that handles it gets to clean up, and kills one that ignores it once the
/// grace has passed. The run ending is the proof of the kill.
#[cfg(unix)]
#[test]
fn recursive_run_bail_stops_running_scripts_gracefully_then_kills_them() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let graceful = r#"node -e "const fs = require('fs'); process.on('SIGTERM', () => { fs.writeFileSync('terminated.txt', ''); process.exit(0) }); fs.writeFileSync('ran.txt', ''); setInterval(() => {}, 1000)""#;
    let stubborn = r#"node -e "const fs = require('fs'); process.on('SIGTERM', () => fs.writeFileSync('terminated.txt', '')); fs.writeFileSync('ran.txt', ''); setInterval(() => {}, 1000)""#;
    let fails = r#"node -e "const fs = require('fs'); const wait = () => fs.existsSync('../a-graceful/ran.txt') && fs.existsSync('../c-stubborn/ran.txt') ? process.exit(1) : setTimeout(wait, 10); wait()""#;
    write_workspace(
        &workspace,
        &[("a-graceful", graceful), ("b-fails", fails), ("c-stubborn", stubborn)],
    );

    let mut run = pacquet
        .with_args(["--workspace-concurrency=3", "--no-sort", "-r", "run", "build"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn pacquet");
    let status = wait_for_child(&mut run);
    assert!(!status.success(), "the failing project should fail the run");
    for name in ["a-graceful", "c-stubborn"] {
        assert!(
            workspace
                .join(name)
                .join("terminated.txt")
                .exists(),
            "{name} should have been asked to stop with SIGTERM before pnpm exited",
        );
    }

    drop(root);
}

/// A bail ends everything a running script started, even a detached process
/// whose parent has exited, which no walk of parent links would find: each
/// script runs in a Job Object of its own.
#[cfg(windows)]
#[test]
fn recursive_run_bail_ends_a_detached_grandchild_of_a_running_script() {
    use std::net::TcpListener;

    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listen for the grandchild");
    listener.set_nonblocking(true).expect("set listener nonblocking");
    let port = listener
        .local_addr()
        .expect("read listener address")
        .port();
    let ready = workspace.join("grandchild-ready.txt");
    // The script starts a child that starts the detached grandchild and
    // exits at once, then keeps running until the bail ends it.
    let starter = super::killed::make_connected_detached_node_script(&ready, port)
        .replace("setInterval(() => {}, 1000)", "process.exit(0)");
    let starter_json = serde_json::to_string(&starter).expect("quote the starter");
    fs::write(
        workspace.join("script.cjs"),
        format!(
            "require('child_process').spawn(process.execPath, ['-e', {starter_json}], {{ stdio: 'ignore' }}); setInterval(() => {{}}, 1000)",
        ),
    )
    .expect("write script.cjs");
    let ready_json = serde_json::to_string(&ready.to_string_lossy()).expect("quote ready path");
    fs::write(
        workspace.join("fail.cjs"),
        format!(
            "const fs = require('fs'); const wait = () => fs.existsSync({ready_json}) ? process.exit(1) : setTimeout(wait, 10); wait()",
        ),
    )
    .expect("write fail.cjs");
    write_workspace(
        &workspace,
        &[("a-running", "node ../script.cjs"), ("b-fails", "node ../fail.cjs")],
    );

    let mut run = pacquet
        .with_args(["--workspace-concurrency=2", "--no-sort", "-r", "run", "build"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn pacquet");
    let connection = super::killed::accept_detached_connection(&listener, &mut run);
    let status = wait_for_child(&mut run);
    assert!(!status.success(), "the failing project should fail the run");
    super::killed::assert_connection_closes(connection);

    drop(root);
}
