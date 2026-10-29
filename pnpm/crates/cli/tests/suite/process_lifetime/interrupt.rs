//! `Ctrl+C` and a closing terminal. The terminal delivers both to every
//! process attached to it, so what pnpm adds must not change which of a
//! command's leftovers survive: a leftover outside the terminal survives
//! both, and one attached to it survives `Ctrl+C`, which it ignores, but
//! not the terminal closing.
//!
//! Unix runs the command as the foreground job of a pseudo-terminal, and
//! Windows on a private console.

use super::{Leftover, launcher_args, launcher_directly, surviving, write_fixture};
#[cfg(windows)]
use crate::_utils::console::PrivateConsole;
#[cfg(unix)]
use crate::_utils::terminal::Terminal;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
#[cfg(windows)]
use std::process::Stdio;
use std::{
    path::Path,
    process::{Child, Command, ExitStatus},
    thread::sleep,
    time::{Duration, Instant},
};

const LEFTOVERS: [Leftover; 2] = [Leftover::Detached, Leftover::Attached];

/// What the terminal does to the running command.
#[derive(Clone, Copy, Debug)]
enum Event {
    CtrlC,
    Close,
}

/// Run the launcher with [`LEFTOVERS`], directly or under `pnpm exec`, let
/// `event` happen once everything is up, and report which leftovers
/// survived.
fn survivors(event: Event, under_pnpm: bool) -> Vec<bool> {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_fixture(&workspace);
    let args = launcher_args("wait", &LEFTOVERS);
    let command = if under_pnpm {
        pacquet
            .with_arg("exec")
            .with_arg(super::node_executable())
            .with_arg("launcher.js")
            .with_args(&args)
    } else {
        launcher_directly(&workspace, &args)
    };
    run_through(command, &workspace, event);
    let survivors = surviving(&workspace, &LEFTOVERS);
    drop(root);
    survivors
}

#[cfg(windows)]
fn run_through(command: Command, workspace: &Path, event: Event) {
    let console = PrivateConsole::attach();
    let mut process = command
        .with_stdin(Stdio::null())
        .with_stdout(Stdio::null())
        .with_stderr(Stdio::null())
        .spawn()
        .expect("spawn the command");
    wait_for_start(&workspace.join("started.txt"), &mut process);
    match event {
        Event::CtrlC => {
            console.press_ctrl_c();
            wait_for_exit(&mut process);
        }
        Event::Close => {
            console.close();
            wait_for_exit(&mut process);
        }
    }
}

#[cfg(unix)]
fn run_through(command: Command, workspace: &Path, event: Event) {
    let terminal = Terminal::open();
    let mut process = terminal.spawn_foreground(command);
    wait_for_start(&workspace.join("started.txt"), &mut process);
    match event {
        Event::CtrlC => terminal.press_ctrl_c(),
        Event::Close => terminal.hang_up(),
    }
    wait_for_exit(&mut process);
    drop(terminal);
}

/// `Ctrl+C` leaves both leftovers running: the detached one never sees it,
/// and the attached one ignores it (pnpm/pnpm#17465).
#[test]
fn ctrl_c_leaves_the_same_processes_running_as_without_pnpm() {
    let without_pnpm = survivors(Event::CtrlC, false);
    assert_eq!(without_pnpm, [true, true], "both leftovers outlive the interrupted command");
    assert_eq!(survivors(Event::CtrlC, true), without_pnpm, "pnpm exec leaves the same processes");
}

/// A closing terminal ends what is attached to it and nothing else.
#[test]
fn a_closing_terminal_leaves_the_same_processes_running_as_without_pnpm() {
    let without_pnpm = survivors(Event::Close, false);
    assert_eq!(
        without_pnpm,
        [true, false],
        "only the detached leftover outlives the closed terminal",
    );
    assert_eq!(survivors(Event::Close, true), without_pnpm, "pnpm exec leaves the same processes");
}

/// A command that ignores `Ctrl+C` is ended by pnpm on the second one, and
/// pnpm then exits on its own, so the command's leftovers keep running. On
/// Unix, `a_third_interrupt_ends_pnpm_even_when_the_script_ignores_them` in
/// `interrupt` covers the escalation that ends pnpm itself instead.
#[cfg(windows)]
#[test]
fn repeated_ctrl_c_ends_a_command_that_ignores_it_but_not_its_leftovers() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_fixture(&workspace);
    let leftovers = [Leftover::Detached];

    let console = PrivateConsole::attach();
    let mut process = pacquet
        .with_arg("exec")
        .with_arg(super::node_executable())
        .with_arg("launcher.js")
        .with_args(launcher_args("hold", &leftovers))
        .with_stdin(Stdio::null())
        .with_stdout(Stdio::null())
        .with_stderr(Stdio::null())
        .spawn()
        .expect("spawn pnpm");
    wait_for_start(&workspace.join("started.txt"), &mut process);
    console.press_ctrl_c();
    sleep(Duration::from_millis(500));
    assert!(
        process
            .try_wait()
            .expect("poll pnpm")
            .is_none(),
        "pnpm waits for a command that ignores the first Ctrl+C",
    );
    console.press_ctrl_c();
    let status = wait_for_exit(&mut process);
    drop(console);

    assert!(!status.success(), "the command pnpm ended fails: {status:?}");
    assert_eq!(surviving(&workspace, &leftovers), [true], "the leftover outlives pnpm");
    drop(root);
}

/// Wait for the launcher to report that everything is up. A command that
/// exits first, or never gets there, fails the test, and is ended so that
/// nothing of it outlives the test.
fn wait_for_start(path: &Path, process: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !path.exists() {
        if let Some(status) = process.try_wait().expect("poll the command") {
            panic!("the command exited before it started its leftovers: {status:?}");
        }
        if Instant::now() >= deadline {
            let _ = process.kill();
            let _ = process.wait();
            panic!("the command never started its leftovers");
        }
        sleep(Duration::from_millis(50));
    }
}

/// Wait for the command to end on its own after the terminal's event.
fn wait_for_exit(process: &mut Child) -> ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = process.try_wait().expect("poll the command") {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = process.kill();
            let _ = process.wait();
            panic!("the command kept running after the terminal's event");
        }
        sleep(Duration::from_millis(50));
    }
}
