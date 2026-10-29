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

/// A Node process that ignores `Ctrl+C` and, once `$env:RELEASE` exists,
/// writes the file the environment variable named by its argument holds. It
/// writes that path with `.up` appended as soon as it runs.
const WAITER_SCRIPT: &str = "const fs = require('fs'); const marker = process.env[process.argv[2]]; process.on('SIGINT', () => {}); fs.writeFileSync(marker + '.up', ''); const deadline = Date.now() + 30000; const wait = () => fs.existsSync(process.env.RELEASE) ? fs.writeFileSync(marker, '') : Date.now() < deadline && setTimeout(wait, 50); wait()";

/// Start one waiter the way `open` starts a browser, through the shell, so
/// it runs outside the console, and one sharing the console. Say when both
/// are up and wait to be interrupted. PowerShell would end a script file
/// with 0 after `Ctrl+C`, so it fails explicitly, as an interrupted Node
/// does.
const OPEN_SCRIPT: &str = r"
$waiter = Join-Path $PSScriptRoot 'waiter.js'
Start-Process -FilePath node -ArgumentList $waiter, 'DETACHED_MARKER' -WindowStyle Hidden
Start-Process -FilePath node -ArgumentList $waiter, 'CONSOLE_MARKER' -NoNewWindow
$deadline = (Get-Date).AddSeconds(30)
while (-not ((Test-Path ($env:DETACHED_MARKER + '.up')) -and (Test-Path ($env:CONSOLE_MARKER + '.up'))) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 50 }
Set-Content -Path 'started.txt' -Value ''
try { Start-Sleep -Seconds 60 } finally { exit 1 }
";

/// Which of [`OPEN_SCRIPT`]'s waiters outlived the command.
#[derive(Debug, PartialEq, Eq)]
struct Survivors {
    outside_the_console: bool,
    sharing_the_console: bool,
}

/// After `Ctrl+C` the leftovers of a command outlive `pnpm exec` exactly
/// when they outlive the command run without pnpm: a program started outside
/// the console, such as the browser `open` starts (pnpm/pnpm#17465), and a
/// process on the console that ignores the interrupt.
#[test]
fn ctrl_c_leaves_the_same_processes_running_as_without_pnpm() {
    let without_pnpm = interrupted_survivors(false);
    assert_eq!(
        without_pnpm,
        Survivors { outside_the_console: true, sharing_the_console: true },
        "both waiters outlive the interrupted command run directly",
    );
    assert_eq!(interrupted_survivors(true), without_pnpm, "pnpm exec leaves the same processes");
}

/// Run [`OPEN_SCRIPT`] at a private console, directly or under `pnpm exec`,
/// press `Ctrl+C` once both waiters are up, and report which outlived it.
fn interrupted_survivors(under_pnpm: bool) -> Survivors {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_project(&workspace);
    fs::write(workspace.join("waiter.js"), WAITER_SCRIPT).expect("write waiter.js");
    fs::write(workspace.join("open.ps1"), OPEN_SCRIPT).expect("write open.ps1");
    let release = workspace.join("release.txt");
    let detached_marker = workspace.join("detached-marker.txt");
    let console_marker = workspace.join("console-marker.txt");
    let script = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File"];

    let console = PrivateConsole::attach();
    let command = if under_pnpm {
        pacquet.with_arg("exec").with_args(script)
    } else {
        let mut powershell = Command::new(script[0]);
        powershell
            .current_dir(&workspace)
            .args(&script[1..]);
        powershell
    };
    let mut process = command
        .with_env("RELEASE", &release)
        .with_env("DETACHED_MARKER", &detached_marker)
        .with_env("CONSOLE_MARKER", &console_marker)
        .with_arg(workspace.join("open.ps1"))
        .with_stdin(Stdio::null())
        .with_stdout(Stdio::null())
        .with_stderr(Stdio::null())
        .spawn()
        .expect("spawn the command");
    wait_for_file(&workspace.join("started.txt"), &mut process);
    console.press_ctrl_c();
    let status = wait_for_shutdown(&mut process);
    drop(console);
    assert!(!status.success(), "the interrupted command fails: {status:?}");

    fs::write(&release, "").expect("release the waiters");
    let outside_the_console = wait_for_marker(&detached_marker);
    let sharing_the_console = wait_for_marker(&console_marker);
    drop(root);
    Survivors { outside_the_console, sharing_the_console }
}

fn wait_for_marker(path: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(50));
    }
    true
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
