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
    write_workspace(&workspace, &[("a-launches", &launching(0)), ("b-succeeds", r#"node -e """#)]);

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
    let stubborn = r#"node -e "const fs = require('fs'); process.on('SIGTERM', () => fs.writeFileSync('terminated.txt', '')); fs.writeFileSync('ran.txt', String(process.pid)); setTimeout(() => process.exit(0), 60000)""#;
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
    let stubborn_dir = workspace.join("c-stubborn");
    let asked_to_stop = stubborn_dir.join("terminated.txt");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !asked_to_stop.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let stubborn: libc::pid_t = fs::read_to_string(stubborn_dir.join("ran.txt"))
        .expect("read the stubborn script's pid")
        .parse()
        .expect("the stubborn script wrote its pid");
    // Well inside the grace: the script that ignored `SIGTERM` is still
    // running, and pnpm still waits for it.
    std::thread::sleep(std::time::Duration::from_secs(1));
    let running_in_grace = is_running(stubborn)
        && run
            .try_wait()
            .expect("poll pnpm")
            .is_none();
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
    assert!(running_in_grace, "the script that ignored SIGTERM gets its grace before the kill");
    // The fixture gives up on its own well after this, so a failure leaves
    // nothing running for long and no recorded pid has to be signalled.
    assert!(
        ends_within(stubborn, std::time::Duration::from_secs(5)),
        "the script that ignored SIGTERM should have been killed after the grace",
    );

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

/// Whether `pid` still runs. A process that has exited but that its new
/// parent has not reaped yet has stopped running.
#[cfg(unix)]
fn is_running(pid: libc::pid_t) -> bool {
    // SAFETY: signal 0 only probes the process the fixture recorded.
    let alive = unsafe { libc::kill(pid, 0) == 0 };
    let zombie = fs::read_to_string(format!("/proc/{pid}/stat"))
        .is_ok_and(|stat| {
            stat.rsplit_once(") ")
                .is_some_and(|(_, rest)| rest.starts_with('Z'))
        });
    alive && !zombie
}

/// Whether `pid` stops running within `deadline`.
#[cfg(unix)]
fn ends_within(pid: libc::pid_t, deadline: std::time::Duration) -> bool {
    let until = std::time::Instant::now() + deadline;
    while is_running(pid) {
        if std::time::Instant::now() >= until {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    true
}

/// After an interrupt pnpm waits for the rest of a script's process group,
/// once the script itself has exited. A bail during that wait still stops
/// what the script left in its group: without that, a process there that
/// ignored the interrupt would keep pnpm waiting for ever.
#[cfg(unix)]
#[test]
fn recursive_run_bail_stops_what_an_interrupted_script_left_in_its_group() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("parent.js"),
        r"const fs = require('fs')
require('child_process').spawn(process.execPath, [require('path').join(__dirname, 'lingering.js')], { stdio: 'ignore' })
fs.writeFileSync('ran.txt', '')
setInterval(() => {}, 1000)
",
    )
    .expect("write parent.js");
    fs::write(
        workspace.join("lingering.js"),
        r"const fs = require('fs')
process.on('SIGINT', () => fs.writeFileSync('interrupted.txt', ''))
process.on('SIGTERM', () => { fs.writeFileSync('terminated.txt', ''); process.exit(0) })
fs.writeFileSync('lingering.txt', '')
setTimeout(() => process.exit(0), 60000)
",
    )
    .expect("write lingering.js");
    let fails = r#"node -e "process.on('SIGINT', () => {}); const fs = require('fs'); const wait = () => fs.existsSync('../a-interrupted/interrupted.txt') ? setTimeout(() => process.exit(1), 300) : setTimeout(wait, 10); wait()""#;
    write_workspace(&workspace, &[("a-interrupted", "node ../parent.js"), ("b-fails", fails)]);

    let mut run = crate::_utils::terminal::spawn_without_terminal(
        pacquet
            .with_args(["--workspace-concurrency=2", "--no-sort", "-r", "run", "build"])
            .with_stdout(Stdio::null())
            .with_stderr(Stdio::null()),
    );
    let lingering = workspace.join("a-interrupted").join("lingering.txt");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !lingering.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let pid = libc::pid_t::try_from(run.id()).expect("pnpm's pid fits");
    // SAFETY: `pid` is the pnpm process this test spawned and still waits on.
    unsafe {
        libc::kill(pid, libc::SIGINT);
    }
    wait_for_child(&mut run);

    assert!(
        workspace
            .join("a-interrupted")
            .join("terminated.txt")
            .exists(),
        "the bail should have stopped what the interrupted script left in its group",
    );
    drop(root);
}
