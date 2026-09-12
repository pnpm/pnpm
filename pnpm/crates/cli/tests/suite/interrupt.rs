#![cfg(unix)]

use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::{
    fs,
    os::unix::process::{CommandExt, ExitStatusExt},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    thread::sleep,
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
fs.writeFileSync('started.txt', '')
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

    let mut process = interruptible(pacquet.with_args(["run", "dev"]));
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

    let mut process = interruptible(pacquet.with_args(["run", "dev"]));
    wait_for_file(&workspace.join("started.txt"), &mut process);
    interrupt(&process);
    let status = wait_for_shutdown(&mut process);

    assert_eq!(status.signal(), Some(libc::SIGUSR2), "pnpm should end the way the script did");

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

    let mut process = interruptible(pacquet.with_args([
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
            workspace.join(project).join("shut-down.txt").exists(),
            "{project} should have finished shutting down before pnpm exited",
        );
    }

    drop(root);
}

/// A script cannot trap the terminal by ignoring what pnpm relays: the
/// third interrupt stops the waiting, and pnpm then ends the way the
/// signal would have ended it all along.
#[test]
fn a_third_interrupt_ends_pnpm_even_when_the_script_ignores_them() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project(&workspace, "test", STUBBORN_SCRIPT);

    // The script outlives pnpm here by design, so its stdio is discarded
    // rather than left holding the test harness's pipes open.
    let mut process = interruptible(
        pacquet.with_args(["run", "dev"]).with_stdout(Stdio::null()).with_stderr(Stdio::null()),
    );
    wait_for_file(&workspace.join("started.txt"), &mut process);
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

    let mut process = interruptible(
        pacquet.with_env("PNPM_CONFIG_ENABLE_PRE_POST_SCRIPTS", "true").with_args(["run", "dev"]),
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

fn write_project(dir: &Path, name: &str, script: &str) {
    let manifest =
        json!({ "name": name, "version": "0.0.0", "scripts": { "dev": "exec node dev.js" } });
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    fs::write(dir.join("dev.js"), script).expect("write the script");
}

fn write_workspace(workspace: &Path, projects: &[&str], script: &str) {
    let packages = projects.iter().map(|name| format!("  - {name}")).collect::<Vec<_>>();
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

/// Spawn pnpm able to receive the interrupt signals, which is the state
/// a terminal hands a foreground command.
///
/// Both halves matter, and both are inherited through `exec`. The test
/// harness may run with `SIGINT` ignored, which pnpm would then keep (as
/// it must under `nohup`) and pass on to the script; and it may run with
/// the signal blocked, which no change of disposition undoes.
fn interruptible(mut command: Command) -> Child {
    // SAFETY: `signal` and `sigprocmask` are async-signal-safe, which is
    // all a `pre_exec` hook between `fork` and `exec` may call.
    unsafe {
        command.pre_exec(|| {
            let mut unblocked: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&raw mut unblocked);
            libc::sigprocmask(libc::SIG_SETMASK, &raw const unblocked, std::ptr::null_mut());
            for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
                libc::signal(signal, libc::SIG_DFL);
            }
            Ok(())
        });
    }
    command.spawn().expect("spawn `pnpm run dev`")
}

/// Send `SIGINT` to pnpm alone, which is what `kill -INT` does; a
/// terminal would signal the whole foreground group at once.
fn interrupt(process: &Child) {
    let pid = i32::try_from(process.id()).expect("the pid fits in a pid_t");
    // SAFETY: `pid` is the child this test spawned and has not waited for
    // yet, so it is not a recycled process id.
    let signalled = unsafe { libc::kill(pid, libc::SIGINT) };
    assert_eq!(signalled, 0, "the interrupt should reach pnpm");
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
