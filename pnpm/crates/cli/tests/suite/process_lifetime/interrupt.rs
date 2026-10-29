//! `Ctrl+C` and a closing terminal. The terminal delivers both to every
//! process attached to it, so what pnpm adds must not change which of a
//! command's leftovers survive: a leftover outside the terminal survives
//! both, and one attached to it survives `Ctrl+C`, which it ignores, but
//! not the terminal closing.
//!
//! Unix runs the command as the foreground job of a pseudo-terminal, under
//! a stand-in for the shell that leads its session, and Windows runs it on a
//! private console.

use super::{Leftover, launcher_args, launcher_directly, surviving, write_fixture};
#[cfg(windows)]
use crate::_utils::console::PrivateConsole;
#[cfg(unix)]
use crate::_utils::terminal::Terminal;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
#[cfg(windows)]
use std::process::{ExitStatus, Stdio};
use std::{
    path::Path,
    process::{Child, Command},
    thread::sleep,
    time::{Duration, Instant},
};

const LEFTOVERS: [Leftover; 2] = [Leftover::Detached, Leftover::Attached];

/// What the terminal does to the running command.
#[derive(Debug, Clone, Copy)]
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
    #[cfg(windows)]
    let mut session = Session::start(command, &workspace);
    #[cfg(unix)]
    let mut session = Session::start(&command, &workspace);
    session.deliver(event);
    session.wait_for_command();
    let survivors = surviving(&workspace, &LEFTOVERS);
    drop(session);
    drop(root);
    survivors
}

/// The command running on a private console.
#[cfg(windows)]
struct Session {
    console: Option<PrivateConsole>,
    command: Child,
}

#[cfg(windows)]
impl Session {
    fn start(command: Command, workspace: &Path) -> Self {
        let console = PrivateConsole::attach();
        let mut command = command
            .with_stdin(Stdio::null())
            .with_stdout(Stdio::null())
            .with_stderr(Stdio::null())
            .spawn()
            .expect("spawn the command");
        wait_for_start(&workspace.join("started.txt"), &mut command);
        Self { console: Some(console), command }
    }

    fn deliver(&mut self, event: Event) {
        let console = self.console.take().expect("the console is still open");
        match event {
            Event::CtrlC => {
                console.press_ctrl_c();
                self.console = Some(console);
            }
            Event::Close => console.close(),
        }
    }

    fn wait_for_command(&mut self) {
        wait_for_exit(&mut self.command);
    }
}

/// Stands in for the interactive shell of a terminal: it leads the session,
/// survives the interrupt of its foreground job, and stays until the test
/// has released the leftovers. The kernel hangs up a terminal's foreground
/// group when its session leader exits, which a shell does not do when its
/// job ends.
#[cfg(unix)]
const SHELL: &str = r"const fs = require('fs')
process.on('SIGINT', () => {})
const [program, ...args] = process.argv.slice(2)
const job = require('child_process').spawn(program, args, { stdio: 'inherit' })
job.on('exit', () => {
  fs.writeFileSync('command-exited.txt', '')
  const deadline = Date.now() + 15000
  const wait = () => fs.existsSync('release.txt') || Date.now() > deadline ? setTimeout(() => process.exit(0), 1000) : setTimeout(wait, 50)
  wait()
})
";

/// The command running as the foreground job of a pseudo-terminal, under a
/// stand-in for the shell ([`SHELL`]).
#[cfg(unix)]
struct Session {
    terminal: Terminal,
    shell: Child,
    exited: std::path::PathBuf,
}

#[cfg(unix)]
impl Session {
    fn start(command: &Command, workspace: &Path) -> Self {
        let script = workspace.join("shell.js");
        std::fs::write(&script, SHELL).expect("write shell.js");
        let mut shell = Command::new(super::node_executable());
        shell
            .current_dir(workspace)
            .arg(script)
            .arg(command.get_program())
            .args(command.get_args());
        for (name, value) in command.get_envs() {
            match value {
                Some(value) => shell.env(name, value),
                None => shell.env_remove(name),
            };
        }
        let terminal = Terminal::open();
        let mut shell = terminal.spawn_foreground(shell);
        wait_for_start(&workspace.join("started.txt"), &mut shell);
        Self { terminal, shell, exited: workspace.join("command-exited.txt") }
    }

    fn deliver(&self, event: Event) {
        match event {
            Event::CtrlC => self.terminal.press_ctrl_c(),
            Event::Close => self.terminal.hang_up(),
        }
    }

    /// Wait for the command to end: the shell says so, unless the hangup
    /// ended the shell too.
    fn wait_for_command(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !self.exited.exists() {
            if self.shell
                .try_wait()
                .expect("poll the shell")
                .is_some()
            {
                return;
            }
            if Instant::now() >= deadline {
                let _ = self.shell.kill();
                let _ = self.shell.wait();
                panic!("the command kept running after the terminal's event");
            }
            sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(unix)]
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.shell.kill();
        let _ = self.shell.wait();
    }
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
#[cfg(windows)]
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
