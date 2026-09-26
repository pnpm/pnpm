//! Signal handling under `pnpm run`: what a `SIGINT` at the terminal
//! reaches, what it leaves behind, and what pnpm's own death leaves behind.
//!
//! Unix-only by subject, not by harness. The tests send POSIX signals to a
//! process group of their own; Windows delivers console control events
//! instead, which needs its own tests rather than a port of these.
#![cfg(unix)]

use crate::_utils::terminal::{Terminal, spawn_without_terminal};
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::{
    fs, io,
    os::unix::process::ExitStatusExt,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    thread::{self, sleep},
    time::{Duration, Instant},
};

/// How long a script is given to shut down before the test gives up. Well
/// past the second the fixtures take, and short enough to report a stuck
/// relay as a failure rather than as a suite-wide timeout.
const SHUTDOWN_DEADLINE: Duration = Duration::from_mins(1);

/// A script that shuts down on `SIGINT` the way a dev server does: not
/// instantly, and writing as it goes.
const GRACEFUL_SCRIPT: &str = r"const fs = require('node:fs')
process.on('SIGINT', () => {
  setTimeout(() => {
    fs.writeFileSync('shut-down.txt', '')
    process.exit(0)
  }, 1000)
})
fs.writeFileSync('started.txt', '')
setInterval(() => {}, 1000)
";

/// A script that shuts down on `SIGINT` and on `SIGTERM` alike, as a
/// server does when a container runtime stops it.
const SIGNAL_SCRIPT: &str = r"const fs = require('node:fs')
for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => {
    setTimeout(() => {
      fs.writeFileSync('shut-down.txt', '')
      process.exit(0)
    }, 1000)
  })
}
fs.writeFileSync('started.txt', '')
setInterval(() => {}, 1000)
";

/// A `pre` script that shuts down on the first interrupt and lets the
/// main script run after it.
const PRE_SCRIPT: &str = r"const fs = require('node:fs')
process.on('SIGINT', () => {
  setTimeout(() => {
    fs.writeFileSync('pre-shut-down.txt', '')
    process.exit(0)
  }, 200)
})
fs.writeFileSync('pre-started.txt', '')
setInterval(() => {}, 1000)
";

/// A script that ignores every signal pnpm relays, so only pnpm's own
/// escalation can end the run. It gives up on its own well after the
/// test, so a failure cannot leave it running for the rest of the suite.
const STUBBORN_SCRIPT: &str = r"const fs = require('node:fs')
process.on('SIGINT', () => {})
process.on('SIGTERM', () => {})
setTimeout(() => process.exit(0), 30_000)
fs.writeFileSync('started.tmp', String(process.pid))
fs.renameSync('started.tmp', 'started.txt')
";

/// A script that records its process id, so a test can tell whether it
/// is still running once pnpm is gone.
const LINGERING_SCRIPT: &str = r"const fs = require('node:fs')
fs.writeFileSync('started.tmp', String(process.pid))
fs.renameSync('started.tmp', 'started.txt')
setInterval(() => {}, 1000)
";

/// A script that reads a repeated interrupt as an order to stop at once,
/// as many CLIs do: the first starts a graceful shutdown, the second
/// forces an exit.
const COUNTING_SCRIPT: &str = r"const fs = require('node:fs')
let interrupts = 0
process.on('SIGINT', () => {
  interrupts += 1
  if (interrupts > 1) {
    fs.writeFileSync('forced.txt', '')
    process.exit(130)
  }
  setTimeout(() => {
    fs.writeFileSync('shut-down.txt', '')
    process.exit(0)
  }, 1000)
})
fs.writeFileSync('started.txt', '')
setInterval(() => {}, 1000)
";

/// A script that turns the interrupt into a different signal, so which
/// one pnpm ends with says whether pnpm read its own signal or the
/// script's outcome.
const RESIGNALLING_SCRIPT: &str = r"const fs = require('node:fs')
process.on('SIGINT', () => {
  process.kill(process.pid, 'SIGUSR2')
})
fs.writeFileSync('started.txt', '')
setInterval(() => {}, 1000)
";

/// An interrupted script keeps the terminal until it has shut down: pnpm
/// stays alive for it, so the shell only gets its prompt back once the
/// script has stopped writing
/// ([pnpm/pnpm#14723](https://github.com/pnpm/pnpm/issues/14723)).
#[test]
fn run_waits_for_the_interrupted_script_to_shut_down() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project(&workspace, "test", GRACEFUL_SCRIPT);

    let mut process = spawn_without_terminal(pacquet.with_args(["run", "dev"]));
    wait_for_file(&workspace.join("started.txt"), &mut process);
    interrupt(&process);
    let status = wait_for_shutdown(&mut process);

    assert!(
        workspace.join("shut-down.txt").exists(),
        "the script must have finished shutting down before pnpm exited",
    );
    assert_eq!(status.code(), Some(0), "the script exited 0, so pnpm does too");

    drop(root);
}

/// A script killed by a signal leaves pnpm no exit code to report, so
/// pnpm re-raises the signal that killed it and the shell sees an
/// interrupted command rather than a plain failure.
#[test]
fn run_ends_with_the_signal_that_killed_the_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project(&workspace, "test", RESIGNALLING_SCRIPT);

    let mut process = spawn_without_terminal(pacquet.with_args(["run", "dev"]));
    wait_for_file(&workspace.join("started.txt"), &mut process);
    interrupt(&process);
    let status = wait_for_shutdown(&mut process);

    assert_eq!(status.signal(), Some(libc::SIGUSR2), "pnpm should end the way the script did");

    drop(root);
}

/// `Ctrl+C` interrupts the terminal's whole foreground group, so the
/// script has the signal by the time pnpm does. pnpm passes nothing on,
/// and the script counts one interrupt rather than two
/// ([pnpm/pnpm#7374](https://github.com/pnpm/pnpm/issues/7374)).
#[test]
fn ctrl_c_interrupts_the_script_once() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project(&workspace, "test", COUNTING_SCRIPT);

    let terminal = Terminal::open();
    let mut process = terminal.spawn_foreground(pacquet.with_args(["run", "dev"]));
    wait_for_file(&workspace.join("started.txt"), &mut process);
    terminal.press_ctrl_c();
    let status = wait_for_shutdown(&mut process);

    assert!(
        !workspace.join("forced.txt").exists(),
        "the script saw a second interrupt, so pnpm relayed the terminal's",
    );
    assert!(
        workspace.join("shut-down.txt").exists(),
        "the script must have finished shutting down before pnpm exited",
    );
    assert_eq!(status.code(), Some(0), "the script exited 0, so pnpm does too");

    drop(root);
}

/// A nested `pnpm run` keeps a shell as the script's parent. dash holds the
/// terminal's `SIGINT` until that command exits, so pnpm's status is the
/// script's own status
/// (<https://github.com/pnpm/pnpm/issues/9945>).
#[test]
fn a_nested_run_keeps_the_scripts_exit_status_after_ctrl_c() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let pnpm = pacquet.get_program().to_owned();
    let bin = root.path().join("bin");
    fs::create_dir_all(&bin).expect("create a bin directory");
    std::os::unix::fs::symlink(&pnpm, bin.join("pnpm")).expect("expose the test pnpm as pnpm");
    let path = std::env::join_paths(
        std::iter::once(bin)
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())),
    )
    .expect("join PATH");
    write_project_running(&workspace, "test", "node dev.js", GRACEFUL_SCRIPT);
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": {
            "start": "node dev.js",
            "start:with-bug": "pnpm run start",
        },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");

    let mut terminal = Terminal::open();
    let mut process = terminal.spawn_foreground(
        pacquet
            .with_env("PATH", path)
            .with_args(["--config.verify-deps-before-run=false", "run", "start:with-bug"]),
    );
    wait_for_file(&workspace.join("started.txt"), &mut process);
    terminal.press_ctrl_c();
    let status = wait_for_shutdown(&mut process);
    let output = terminal.captured_output();

    assert!(
        workspace.join("shut-down.txt").exists(),
        "the script must have finished shutting down before pnpm exited\n{output}",
    );
    assert!(
        !output.contains("ELIFECYCLE"),
        "a script that shut down cleanly is not a lifecycle failure\n{output}",
    );
    assert_eq!(status.code(), Some(0), "the script exited 0, so pnpm does too\n{output}");

    drop(root);
}

/// dash dies from a terminal `SIGINT` that the foreground command handled;
/// bash carries on with the rest of the script. Every shell now does what
/// bash does.
#[test]
fn ctrl_c_handled_by_a_command_lets_the_rest_of_the_script_run() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project_running(&workspace, "test", "node dev.js && echo > after.txt", GRACEFUL_SCRIPT);

    let mut terminal = Terminal::open();
    let mut process = terminal.spawn_foreground(pacquet.with_args([
        "--config.verify-deps-before-run=false",
        "run",
        "dev",
    ]));
    wait_for_file(&workspace.join("started.txt"), &mut process);
    terminal.press_ctrl_c();
    let status = wait_for_shutdown(&mut process);
    let output = terminal.captured_output();

    assert!(
        workspace.join("after.txt").exists(),
        "the command after the interrupted one must run\n{output}",
    );
    assert_eq!(status.code(), Some(0), "the script exited 0, so pnpm does too\n{output}");

    drop(root);
}

/// The same shell, when the script never handles `SIGINT`, still ends
/// pnpm with that signal. The child's status is a real interrupt, and
/// pnpm reports it.
#[test]
fn ctrl_c_still_reports_a_script_the_shell_could_not_keep_alive() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project_running(&workspace, "test", "node dev.js", LINGERING_SCRIPT);

    let mut terminal = Terminal::open();
    let mut process = terminal.spawn_foreground(pacquet.with_args([
        "--config.verify-deps-before-run=false",
        "run",
        "dev",
    ]));
    wait_for_file(&workspace.join("started.txt"), &mut process);
    terminal.press_ctrl_c();
    let status = wait_for_shutdown(&mut process);
    let output = terminal.captured_output();

    assert_eq!(
        status.signal(),
        Some(libc::SIGINT),
        "an unhandled interrupt still ends pnpm with SIGINT\n{output}",
    );
    assert!(
        output.contains("[ELIFECYCLE] Command failed with signal SIGINT."),
        "an unhandled interrupt is still a lifecycle failure\n{output}",
    );

    drop(root);
}

/// Without a terminal, the shell running the script may stay its parent
/// (dash does) and then keeps a relayed `SIGINT` to itself until its
/// child exits. pnpm signals the script's whole process group instead,
/// and waits for the group, so the script shuts down and finishes before
/// pnpm ends.
#[test]
fn a_shell_that_stays_the_scripts_parent_passes_the_interrupt_on() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project_running(&workspace, "test", "node dev.js", SIGNAL_SCRIPT);

    let mut process = spawn_without_terminal(pacquet.with_args(["run", "dev"]));
    wait_for_file(&workspace.join("started.txt"), &mut process);
    interrupt(&process);
    wait_for_shutdown(&mut process);

    assert!(
        workspace.join("shut-down.txt").exists(),
        "the script must have finished shutting down before pnpm exited",
    );

    drop(root);
}

/// A `SIGTERM`, which is how a container runtime or a service manager stops
/// pnpm, ends a shell that stays the script's parent at once. pnpm still
/// signals the script through its process group and waits for it, so the
/// script's shutdown completes before pnpm ends.
#[test]
fn a_termination_without_a_terminal_reaches_the_script_behind_its_shell() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project_running(&workspace, "test", "node dev.js", SIGNAL_SCRIPT);

    let mut process = spawn_without_terminal(pacquet.with_args(["run", "dev"]));
    wait_for_file(&workspace.join("started.txt"), &mut process);
    signal(&process, libc::SIGTERM);
    wait_for_shutdown(&mut process);

    assert!(
        workspace.join("shut-down.txt").exists(),
        "the script must have finished shutting down before pnpm exited",
    );

    drop(root);
}

/// A recursive run gives each project's script a process group of its
/// own, which the terminal's signals never reach. pnpm addresses those
/// groups, so every project shuts down instead of being left behind.
#[test]
fn a_parallel_run_relays_the_interrupt_to_every_project() {
    const PROJECTS: [&str; 2] = ["project-1", "project-2"];

    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace(&workspace, &PROJECTS, GRACEFUL_SCRIPT);

    let mut process = spawn_without_terminal(pacquet.with_args([
        "-r",
        "--filter=./project-*",
        "--parallel",
        "run",
        "dev",
    ]));
    for project in PROJECTS {
        wait_for_file(&workspace.join(project).join("started.txt"), &mut process);
    }
    interrupt(&process);
    wait_for_shutdown(&mut process);

    for project in PROJECTS {
        assert!(
            workspace
                .join(project)
                .join("shut-down.txt")
                .exists(),
            "{project} should have finished shutting down before pnpm exited",
        );
    }

    drop(root);
}

/// A script cannot trap the terminal by ignoring what pnpm relays: the
/// third interrupt stops the waiting, and pnpm then ends the way the
/// signal would have ended it all along. The script, which nothing but
/// pnpm could reach, ends with it.
#[test]
fn a_third_interrupt_ends_pnpm_even_when_the_script_ignores_them() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project(&workspace, "test", STUBBORN_SCRIPT);

    // The script outlives pnpm for a moment, so its stdio is discarded
    // rather than left holding the test harness's pipes open should it
    // outlive pnpm for good.
    let mut process = spawn_without_terminal(
        pacquet
            .with_args(["run", "dev"])
            .with_stdout(Stdio::null())
            .with_stderr(Stdio::null()),
    );
    wait_for_file(&workspace.join("started.txt"), &mut process);
    let script = read_pid(&workspace.join("started.txt"));
    for _ in 0..3 {
        interrupt(&process);
        // Signals do not queue, so each has to be taken before the next.
        sleep(Duration::from_millis(200));
    }
    let status = wait_for_shutdown(&mut process);

    assert_eq!(
        status.signal(),
        Some(libc::SIGINT),
        "pnpm should end as an unhandled interrupt would, not with a plain exit code",
    );
    // Well before the script gives up on its own, so only the watchdog can
    // satisfy this.
    assert!(ends_within(script, Duration::from_secs(10)), "the script should not outlive pnpm");

    drop(root);
}

/// A tool that starts `pnpm run` detached and stops it by killing its
/// process group, as Playwright's `webServer` does, reaches pnpm but not
/// a script in a group of its own. The script must still end with pnpm:
/// a survivor keeps the tool's output pipes open, and the tool waits on
/// them for ever
/// ([pnpm/pnpm#15555](https://github.com/pnpm/pnpm/issues/15555)).
#[test]
fn killing_the_process_group_of_pnpm_kills_the_script_behind_its_shell_too() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project_running(&workspace, "test", "node dev.js", LINGERING_SCRIPT);

    let mut process = spawn_without_terminal(
        pacquet
            .with_args(["run", "dev"])
            .with_stdout(Stdio::piped())
            .with_stderr(Stdio::piped()),
    );
    let stdout = process.stdout.take().expect("stdout is piped");
    let stderr = process.stderr.take().expect("stderr is piped");
    wait_for_file(&workspace.join("started.txt"), &mut process);
    let script = read_pid(&workspace.join("started.txt"));
    signal_group(&process, libc::SIGKILL);
    let status = wait_for_shutdown(&mut process);

    assert_eq!(status.signal(), Some(libc::SIGKILL), "the kill should have reached pnpm");
    let stdout_closed = closes_within(stdout, SHUTDOWN_DEADLINE);
    let stderr_closed = closes_within(stderr, SHUTDOWN_DEADLINE);
    if !stdout_closed || !stderr_closed {
        // SAFETY: `script` is the process the test's own fixture recorded.
        unsafe {
            libc::kill(script, libc::SIGKILL);
        }
    }
    assert!(stdout_closed, "the script kept pnpm's stdout pipe open after pnpm was killed");
    assert!(stderr_closed, "the script kept pnpm's stderr pipe open after pnpm was killed");
    assert!(ends_within(script, SHUTDOWN_DEADLINE), "the script should not outlive pnpm");

    drop(root);
}

/// pnpm's own exit is not its death. A process the script started and
/// left behind in its group runs on afterwards, as it does when the
/// script shares pnpm's group.
#[test]
fn a_process_the_script_left_behind_survives_the_exit_of_pnpm() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project_running(
        &workspace,
        "test",
        "node dev.js </dev/null >/dev/null 2>&1 &",
        LINGERING_SCRIPT,
    );

    let mut process = spawn_without_terminal(pacquet.with_args(["run", "dev"]));
    let status = wait_for_shutdown(&mut process);
    assert_eq!(status.code(), Some(0), "the script backgrounds its work and exits at once");
    let started = workspace.join("started.txt");
    assert!(wait_until(|| started.exists()), "the process left behind should have started");
    let left_behind = read_pid(&started);
    // A watchdog that mistook pnpm's exit for its death would have struck
    // by now.
    sleep(Duration::from_millis(500));
    let running = is_running(left_behind);
    // SAFETY: `left_behind` is the process the test's own fixture recorded.
    unsafe {
        libc::kill(left_behind, libc::SIGKILL);
    }
    assert!(running, "pnpm's exit should not end a process the script left behind");

    drop(root);
}

/// Each script gets its own first interrupt. The escalation counts one
/// burst of them, so an earlier Ctrl+C cannot make the next script's
/// interrupt arrive as the forced `SIGTERM` of a second press.
#[test]
fn a_later_script_still_gets_a_plain_first_interrupt() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "predev": "exec node pre.js", "dev": "exec node dev.js" },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
    fs::write(workspace.join("pre.js"), PRE_SCRIPT).expect("write the pre script");
    fs::write(workspace.join("dev.js"), GRACEFUL_SCRIPT).expect("write the script");

    let mut process = spawn_without_terminal(
        pacquet
            .with_env("PNPM_CONFIG_ENABLE_PRE_POST_SCRIPTS", "true")
            .with_args(["run", "dev"]),
    );
    wait_for_file(&workspace.join("pre-started.txt"), &mut process);
    interrupt(&process);
    wait_for_file(&workspace.join("started.txt"), &mut process);
    interrupt(&process);
    let status = wait_for_shutdown(&mut process);

    assert!(
        workspace.join("shut-down.txt").exists(),
        "the main script should have handled an interrupt, not been terminated outright",
    );
    assert_eq!(status.code(), Some(0), "the script exited 0, so pnpm does too");

    drop(root);
}

/// The `dev` script execs `script`: the shell is out of the picture, and
/// the relay's target is the script itself.
fn write_project(dir: &Path, name: &str, script: &str) {
    write_project_running(dir, name, "exec node dev.js", script);
}

fn write_project_running(dir: &Path, name: &str, command: &str, script: &str) {
    let manifest = json!({ "name": name, "version": "0.0.0", "scripts": { "dev": command } });
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    fs::write(dir.join("dev.js"), script).expect("write the script");
}

fn write_workspace(workspace: &Path, projects: &[&str], script: &str) {
    let packages = projects
        .iter()
        .map(|name| format!("  - {name}"))
        .collect::<Vec<_>>();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("packages:\n{}\n", packages.join("\n")),
    )
    .expect("write pnpm-workspace.yaml");
    for name in projects {
        let dir = workspace.join(name);
        fs::create_dir_all(&dir).expect("create the project directory");
        write_project(&dir, name, script);
    }
}

/// Send `SIGINT` to pnpm alone, which is what `kill -INT` does; a
/// terminal would signal the whole foreground group at once.
fn interrupt(process: &Child) {
    signal(process, libc::SIGINT);
}

/// Send `signal` to pnpm alone, as `kill` does.
fn signal(process: &Child, signal: libc::c_int) {
    let pid = i32::try_from(process.id()).expect("the pid fits in a pid_t");
    // SAFETY: `pid` is the child this test spawned and has not waited for
    // yet, so it is not a recycled process id.
    let signalled = unsafe { libc::kill(pid, signal) };
    assert_eq!(signalled, 0, "the signal should reach pnpm");
}

/// Send `signal` to the process group pnpm leads, as a tool that started
/// pnpm detached does to stop it.
fn signal_group(process: &Child, signal: libc::c_int) {
    let pid = i32::try_from(process.id()).expect("the pid fits in a pid_t");
    // SAFETY: `pid` leads the group of the child this test spawned and has
    // not waited for yet, so it is not a recycled process id.
    let signalled = unsafe { libc::kill(-pid, signal) };
    assert_eq!(signalled, 0, "the signal should reach pnpm's process group");
}

/// The process id a fixture script wrote to `path`. The fixtures write it
/// to a temporary file and rename that into place, so a file that exists
/// already holds the whole id.
fn read_pid(path: &Path) -> libc::pid_t {
    fs::read_to_string(path)
        .expect("read the recorded pid")
        .trim()
        .parse()
        .expect("the fixture recorded its pid")
}

/// Whether `pid` still names a live process. One the kernel no longer
/// knows is gone, and so is one that has exited and only waits to be
/// reaped; one that refuses the probe is still there.
fn is_running(pid: libc::pid_t) -> bool {
    // SAFETY: a signal of 0 only probes for the process.
    if unsafe { libc::kill(pid, 0) } != 0 {
        return match io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => false,
            Some(libc::EPERM) => true,
            _ => panic!("probe process {pid}: {}", io::Error::last_os_error()),
        };
    }
    !is_zombie(pid)
}

/// Whether `pid` has exited and waits for a parent to reap it. An orphan
/// stays one until init takes it over, which the probe above cannot tell
/// from a running process.
fn is_zombie(pid: libc::pid_t) -> bool {
    let state = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .expect("run ps");
    state.stdout.trim_ascii_start().first() == Some(&b'Z')
}

/// Whether `pid` is gone before `deadline` passes.
fn ends_within(pid: libc::pid_t, deadline: Duration) -> bool {
    let deadline = Instant::now() + deadline;
    while Instant::now() < deadline {
        if !is_running(pid) {
            return true;
        }
        sleep(Duration::from_millis(20));
    }
    false
}

/// Whether `stream` reaches its end before `deadline` passes, which it
/// does once no process holds it open any more.
fn closes_within(mut stream: impl io::Read + Send + 'static, deadline: Duration) -> bool {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let drained = io::copy(&mut stream, &mut io::sink());
        let _ = sender.send(drained);
    });
    receiver.recv_timeout(deadline).is_ok()
}

/// Wait until `condition` holds, for as long as a script is given to
/// start.
fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        sleep(Duration::from_millis(20));
    }
    false
}

/// Wait for pnpm to end, so a relay that never reached the script fails
/// the test with its own message instead of hanging the whole suite.
fn wait_for_shutdown(process: &mut Child) -> ExitStatus {
    let deadline = Instant::now() + SHUTDOWN_DEADLINE;
    while Instant::now() < deadline {
        if let Some(status) = process.try_wait().expect("poll pnpm") {
            return status;
        }
        sleep(Duration::from_millis(20));
    }
    let _ = process.kill();
    let _ = process.wait();
    panic!("pnpm was still running {SHUTDOWN_DEADLINE:?} after the interrupt");
}

/// Wait until the script announces itself, so the interrupt cannot land
/// before there is a child to relay it to.
fn wait_for_file(path: &Path, process: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if path.exists() {
            return;
        }
        if let Some(status) = process.try_wait().expect("poll pnpm") {
            panic!("pnpm exited with {status} before the script started");
        }
        sleep(Duration::from_millis(20));
    }
    let _ = process.kill();
    let _ = process.wait();
    panic!("the script did not start before the deadline");
}
