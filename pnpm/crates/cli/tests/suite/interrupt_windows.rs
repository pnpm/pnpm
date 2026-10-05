//! `Ctrl+C` under `pnpm run` on Windows. The console delivers the event to
//! every process attached to it at once, so the script, the shell running
//! it and pnpm all have it by the time pnpm's handler runs. The status the
//! shell then leaves is its own: `cmd` ends with [`STATUS_CONTROL_C_EXIT`]
//! once the command it ran has returned, PowerShell with 1. pnpm ends the
//! same way and reports no lifecycle failure for a script the user ended
//! (pnpm/pnpm#16579).
//!
//! Windows-only by subject. The tests in `interrupt` send POSIX signals,
//! which Windows does not have.
#![cfg(windows)]

use crate::_utils::console::PrivateConsole;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::{
    fs,
    io::Read,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    thread::sleep,
    time::{Duration, Instant},
};

/// The status of a process the console's `Ctrl+C` ended, which `cmd`
/// reports once the command it was running has.
const STATUS_CONTROL_C_EXIT: i32 = 0xC000_013A_u32 as i32;

/// A script that runs until something ends it, and says when it is up.
const DEV_SCRIPT: &str =
    r#"node -e "require('node:fs').writeFileSync('started.txt', ''); setInterval(() => {}, 1000)""#;

/// The default script shell, `cmd`, ends with [`STATUS_CONTROL_C_EXIT`] once
/// the command the console interrupted has returned, and pnpm ends with
/// that status. The interrupt is the user's doing, so pnpm prints no
/// `[ELIFECYCLE]` line for it.
#[test]
fn ctrl_c_ends_pnpm_like_the_script_shell_without_a_lifecycle_failure() {
    let (status, output) = interrupt_run(&[]);
    assert_eq!(status.code(), Some(STATUS_CONTROL_C_EXIT), "pnpm ends as cmd did\n{output}");
    assert!(
        !output.contains("ELIFECYCLE"),
        "a script the user interrupted is not a failure\n{output}",
    );
}

/// PowerShell as the script shell ends with 1 after the console's
/// `Ctrl+C`. That is the shell's status, not the script's, and pnpm
/// reports no lifecycle failure for it either.
#[test]
fn ctrl_c_under_a_powershell_script_shell_reports_no_lifecycle_failure() {
    let (status, output) = interrupt_run(&["--config.script-shell=powershell"]);
    assert_eq!(status.code(), Some(1), "pnpm ends as PowerShell did\n{output}");
    assert!(
        !output.contains("ELIFECYCLE"),
        "a script the user interrupted is not a failure\n{output}",
    );
}

/// Run `pnpm run dev` with `args` ahead of the command, press `Ctrl+C` at
/// a private console once the script is up, and return how pnpm ended
/// together with everything it wrote.
fn interrupt_run(args: &[&str]) -> (ExitStatus, String) {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project(&workspace);

    let console = PrivateConsole::attach();
    let mut process = pacquet
        .with_args(args)
        .with_args(["--config.verify-deps-before-run=false", "run", "dev"])
        .with_stdin(Stdio::null())
        .with_stdout(Stdio::piped())
        .with_stderr(Stdio::piped())
        .spawn()
        .expect("spawn pnpm");
    wait_for_file(&workspace.join("started.txt"), &mut process);
    console.press_ctrl_c();
    let status = wait_for_shutdown(&mut process);

    let mut output = String::new();
    for stream in [
        process.stdout
            .take()
            .map(|stream| Box::new(stream) as Box<dyn Read>),
        process.stderr
            .take()
            .map(|stream| Box::new(stream) as Box<dyn Read>),
    ] {
        stream
            .expect("piped stream")
            .read_to_string(&mut output)
            .expect("read pnpm's output");
    }
    eprintln!("status: {status:?}\n{output}");
    drop(console);
    drop(root);
    (status, output)
}

fn write_project(dir: &Path) {
    let manifest = json!({
        "name": "test",
        "version": "0.0.0",
        "scripts": { "dev": DEV_SCRIPT },
    });
    fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
}

/// Wait for the script to write `path`. A pnpm that exits first, or never
/// starts the script, fails the test, with its process tree ended so that
/// nothing of it outlives the test.
fn wait_for_file(path: &Path, process: &mut Child) {
    let deadline = Instant::now() + Duration::from_mins(1);
    while !path.exists() {
        if process
            .try_wait()
            .expect("poll pnpm")
            .is_some()
        {
            panic!("pnpm exited before the script ran");
        }
        if Instant::now() >= deadline {
            let status = end_process_tree(process);
            panic!("the script never started: {status:?}");
        }
        sleep(Duration::from_millis(50));
    }
}

/// Wait for pnpm to end on its own, and end its process tree when it does
/// not: a prompt left on the console would otherwise hold the test.
fn wait_for_shutdown(process: &mut Child) -> ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = process.try_wait().expect("poll pnpm") {
            return status;
        }
        if Instant::now() >= deadline {
            let status = end_process_tree(process);
            panic!("pnpm kept running after Ctrl+C: {status:?}");
        }
        sleep(Duration::from_millis(50));
    }
}

/// End pnpm together with everything it started, and reap it.
fn end_process_tree(process: &mut Child) -> ExitStatus {
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &process.id().to_string()])
        .status();
    process.wait().expect("wait for pnpm")
}
