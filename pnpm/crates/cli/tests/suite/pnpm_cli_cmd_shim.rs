//! The `pnpm.cmd` shim a global install links for the pnpm CLI itself, run
//! the ways Windows users run it: from cmd.exe and from PowerShell.
//!
//! That shim ends its batch context before `pnpm.exe` starts, so that Ctrl+C
//! does not leave cmd.exe asking `Terminate batch job (Y/N)?`. These tests
//! check that the exit code, the arguments, and stdin still reach and leave
//! `pnpm.exe` as they did through an ordinary batch file.
//!
//! Windows-only by subject: there is no `.cmd` shim, and no batch job, anywhere
//! else.
#![cfg(windows)]

use crate::_utils::console::PrivateConsole;
use assert_cmd::cargo::CommandCargoExt;
use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, command_env::CommandTestExt};
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread::sleep,
    time::{Duration, Instant},
};
use tempfile::TempDir;
use windows_sys::Win32::System::{
    Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, GenerateConsoleCtrlEvent},
    Threading::CREATE_NEW_PROCESS_GROUP,
};

/// A script that prints its arguments, one JSON array per run.
const PRINT_ARGS_SCRIPT: &str = "console.log(JSON.stringify(process.argv.slice(2)))\n";

/// Arguments cmd.exe hands over without reinterpreting them once quoted: the
/// option terminator, spaces, an empty string, cmd.exe's metacharacters, a
/// delayed-expansion marker, and non-ASCII text.
const CMD_ARGS: &[&str] =
    &["--", "--flag", "a b", "", "50%", "x^y", "p&q", "r|s", "!bang!", "héllo"];

/// The pnpm CLI installed globally under the package name `pnpm`, as
/// `pnpm setup` and `pnpm self-update` install it, into a `PNPM_HOME` whose
/// path is not ASCII.
struct GlobalCli {
    root: TempDir,
    workspace: PathBuf,
    pnpm_home: PathBuf,
}

impl GlobalCli {
    fn install() -> Self {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        let pnpm_home = root.path().join("pnpm-hömé");
        let package_dir = root.path().join("cli");
        write_cli_package(&package_dir, "12.0.0");
        fs::create_dir_all(pnpm_home.join("bin")).expect("create the global bin dir");
        // A global install anchors its config at the pnpm home, so the
        // per-test store and cache go there.
        fs::write(
            pnpm_home.join("pnpm-workspace.yaml"),
            format!(
                "storeDir: {}\ncacheDir: {}\nenableGlobalVirtualStore: false\n",
                root.path().join("store").display(),
                root.path().join("cache").display(),
            ),
        )
        .expect("seed the pnpm-home workspace yaml");
        fs::write(
            workspace.join("package.json"),
            json!({
                "name": "project",
                "version": "1.0.0",
                "scripts": {
                    "exit-3": r#"node -e "process.exit(3)""#,
                    "exit-0": r#"node -e """#,
                    "print-args": "node print-args.js",
                },
            })
            .to_string(),
        )
        .expect("write the project manifest");
        fs::write(workspace.join("print-args.js"), PRINT_ARGS_SCRIPT)
            .expect("write the argument printer");

        let cli = GlobalCli { root, workspace, pnpm_home };
        let output = cli
            .isolated(Command::cargo_bin("pnpm").expect("find the pnpm binary"))
            .with_args(["add", "-g", "--ignore-scripts"])
            .with_arg(format!("file:{}", package_dir.display()))
            .output()
            .expect("run pnpm add -g");
        assert!(output.status.success(), "pnpm add -g failed: {output:?}");
        assert!(cli.shim().is_file(), "the global install must link {}", cli.shim().display());
        cli
    }

    fn shim(&self) -> PathBuf {
        self.pnpm_home.join("bin").join("pnpm.cmd")
    }

    fn isolated(&self, command: Command) -> Command {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut path_entries = vec![self.pnpm_home.join("bin")];
        path_entries.extend(std::env::split_paths(&path));
        command
            .without_ambient_pnpm_config()
            .with_current_dir(&self.workspace)
            .with_env("PNPM_HOME", &self.pnpm_home)
            .with_env("PATH", std::env::join_paths(path_entries).expect("join PATH"))
            .with_env("XDG_STATE_HOME", self.root.path().join("state"))
            .with_env("XDG_CONFIG_HOME", self.root.path().join("config"))
            .with_env("XDG_CACHE_HOME", self.root.path().join("cache-home"))
    }

    /// `cmd /d /c "<pnpm.cmd>" <command line>`, the command line passed to
    /// cmd.exe as written.
    fn via_cmd(&self, command_line: &str) -> Command {
        let mut command = self.isolated(Command::new("cmd.exe"));
        command.raw_arg(format!(r#"/d /c ""{}" {command_line}""#, self.shim().display()));
        command
    }

    /// `powershell -NoProfile -Command "& '<pnpm.cmd>' <arguments>"`, ending
    /// with the shim's `$LASTEXITCODE`.
    fn via_powershell(&self, arguments: &str) -> Command {
        let script = format!("& '{}' {arguments}; exit $LASTEXITCODE", self.shim().display());
        self.isolated(Command::new("powershell.exe"))
            .with_args(["-NoProfile", "-NonInteractive", "-Command", &script])
    }

    /// `pnpm.exe` itself, the shim's target, with `args` as given.
    fn direct(&self, args: &[&str]) -> Command {
        self.isolated(Command::new(cargo_bin_pnpm()))
            .with_args(args)
    }
}

fn cargo_bin_pnpm() -> PathBuf {
    assert_cmd::cargo::cargo_bin("pnpm")
}

/// The pnpm CLI as the package `pnpm` at `version`, with this build as its
/// executable.
fn write_cli_package(package_dir: &Path, version: &str) {
    fs::create_dir_all(package_dir).expect("create the CLI package dir");
    fs::copy(cargo_bin_pnpm(), package_dir.join("pnpm.exe")).expect("copy pnpm.exe");
    fs::write(
        package_dir.join("package.json"),
        json!({
            "name": "pnpm",
            "version": version,
            "bin": { "pnpm": "pnpm.exe", "pn": "pnpm.exe" },
            "files": ["pnpm.exe"],
        })
        .to_string(),
    )
    .expect("write the CLI package manifest");
}

/// The quoted `pnpm.exe` path on the shim's target line.
fn quoted_target(shim: &str) -> &str {
    let start = shim.rfind(r#""%~dp0\"#).expect("the shim quotes its target");
    let end = shim[start..].find(r#".exe""#).expect("the target is pnpm.exe");
    &shim[start..start + end + r#".exe""#.len()]
}

fn run(mut command: Command) -> Output {
    let output = command.output().expect("spawn the command");
    eprintln!("{command:?}\n{output:?}");
    output
}

/// The JSON argument arrays `print-args.js` printed, in order.
fn printed_args(output: &Output) -> Vec<Vec<String>> {
    String::from_utf8(output.stdout.clone())
        .expect("stdout is UTF-8")
        .lines()
        .filter(|line| line.starts_with('['))
        .map(|line| serde_json::from_str(line).expect("parse printed arguments"))
        .collect()
}

/// `cmd.exe` quoting for `arg`, as a user types it.
fn cmd_quoted(arg: &str) -> String {
    format!(r#""{arg}""#)
}

#[test]
fn the_exit_code_of_a_script_leaves_the_shim_unchanged() {
    let cli = GlobalCli::install();

    for (script, code) in [("exit-3", 3), ("exit-0", 0)] {
        let from_cmd = run(cli.via_cmd(&format!("run {script}")));
        assert_eq!(from_cmd.status.code(), Some(code), "cmd /c pnpm run {script}");

        let from_powershell = run(cli.via_powershell(&format!("run {script}")));
        assert_eq!(from_powershell.status.code(), Some(code), "PowerShell $LASTEXITCODE");

        // `%ERRORLEVEL%` as cmd.exe sees it after the shim returns, which a
        // batch file of the user's reads through `call`. The batch file finds
        // `pnpm` on `PATH`, as a user's would, so its own text stays ASCII.
        let driver = cli.root.path().join("errorlevel.cmd");
        fs::write(&driver, format!("@call pnpm run {script}\r\n@echo errorlevel=%ERRORLEVEL%\r\n"))
            .expect("write the errorlevel driver");
        let mut command = cli.isolated(Command::new("cmd.exe"));
        command.raw_arg(format!(r#"/d /c "{}""#, driver.display()));
        let from_batch = run(command);
        assert!(
            String::from_utf8_lossy(&from_batch.stdout).contains(&format!("errorlevel={code}")),
            "%ERRORLEVEL% after `call pnpm run {script}`: {from_batch:?}",
        );
    }
}

#[test]
fn arguments_reach_pnpm_as_they_do_without_the_shim() {
    let cli = GlobalCli::install();
    let expected = {
        let mut args = vec!["run", "print-args"];
        args.extend(CMD_ARGS);
        printed_args(&run(cli.direct(&args)))
    };
    assert_eq!(expected.len(), 1, "pnpm.exe must print the arguments once");

    let quoted: Vec<String> = CMD_ARGS
        .iter()
        .copied()
        .map(cmd_quoted)
        .collect();
    let from_cmd = run(cli.via_cmd(&format!("run print-args {}", quoted.join(" "))));
    assert!(from_cmd.status.success());
    assert_eq!(printed_args(&from_cmd), expected);

    let exec_args: Vec<&str> = ["exec", "node", "print-args.js"]
        .into_iter()
        .chain(CMD_ARGS.iter().copied())
        .collect();
    let exec_expected = printed_args(&run(cli.direct(&exec_args)));
    let from_cmd = run(cli.via_cmd(&format!("exec node print-args.js {}", quoted.join(" "))));
    assert_eq!(printed_args(&from_cmd), exec_expected);
}

/// PowerShell builds the command line for a batch file itself, and quotes an
/// argument only when it has whitespace. The arguments here are the ones that
/// survive that unchanged.
#[test]
fn arguments_from_powershell_reach_pnpm() {
    let cli = GlobalCli::install();
    let args = ["--", "--flag", "a b", "50%", "!bang!", "p & q", "héllo"];
    let expected = {
        let mut direct_args = vec!["run", "print-args"];
        direct_args.extend(args);
        printed_args(&run(cli.direct(&direct_args)))
    };
    let quoted: Vec<String> = args
        .iter()
        .map(|arg| format!("'{arg}'"))
        .collect();
    let from_powershell = run(cli.via_powershell(&format!("run print-args {}", quoted.join(" "))));
    assert!(from_powershell.status.success());
    assert_eq!(printed_args(&from_powershell), expected);
}

#[test]
fn stdin_reaches_the_command_pnpm_runs() {
    let cli = GlobalCli::install();
    let pipe_through = r#"exec node -e "process.stdin.pipe(process.stdout)""#;

    let mut child = cli
        .via_cmd(pipe_through)
        .with_stdin(Stdio::piped())
        .with_stdout(Stdio::piped())
        .spawn()
        .expect("spawn cmd");
    child.stdin
        .take()
        .expect("child stdin")
        .write_all(b"piped-through-cmd")
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait for cmd");
    eprintln!("{output:?}");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "piped-through-cmd");

    let from_powershell = run(cli
        .isolated(Command::new("powershell.exe"))
        .with_args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "'piped-through-powershell' | & '{}' {pipe_through}; exit $LASTEXITCODE",
                cli.shim().display(),
            ),
        ]));
    assert!(from_powershell.status.success());
    assert!(
        String::from_utf8_lossy(&from_powershell.stdout).contains("piped-through-powershell"),
        "stdin piped from PowerShell must reach the command",
    );
}

/// pnpm 12.8 and older linked a `pnpm.cmd` that cmd.exe is still reading
/// while `pnpm.exe` runs. An update of the CLI from that shim, which is what
/// `pnpm self-update` does, replaces the file while cmd.exe is still reading
/// it. cmd.exe then reads on in the new file from where the old one's target
/// line ended, which must neither run a piece of the new shim nor the update
/// again (pnpm/pnpm#16573).
#[test]
fn updating_the_cli_from_a_batch_kept_shim_runs_pnpm_once() {
    let cli = GlobalCli::install();
    let shim = fs::read_to_string(cli.shim()).expect("read pnpm.cmd");
    let target = quoted_target(&shim);
    let batch_kept = format!("@SETLOCAL\r\n@{target}  %*\r\n");
    fs::write(cli.shim(), &batch_kept).expect("write the batch-kept shim");

    let update = cli.root.path().join("cli-update");
    write_cli_package(&update, "12.0.1");
    let output =
        run(cli.via_cmd(&format!(r#"add -g --ignore-scripts "file:{}""#, update.display())));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{output:?}");
    assert!(!stderr.contains("is not recognized"), "cmd.exe ran a line of the new shim:\n{stderr}");
    assert_eq!(stdout.matches("Done in ").count(), 1, "pnpm ran more than once:\n{stdout}");

    let replaced = fs::read_to_string(cli.shim()).expect("read the replaced pnpm.cmd");
    assert_ne!(replaced, batch_kept, "the update must replace the shim");
    assert!(replaced.contains("@GOTO #_undefined_# 2>NUL || "), "{replaced}");
    assert!(replaced.contains("@EXIT /B %ERRORLEVEL%\r\n"), "{replaced}");

    // The replaced shim, run from its start, still ends its batch before
    // `pnpm.exe` and passes the exit code through.
    let from_cmd = run(cli.via_cmd("run exit-3"));
    assert_eq!(from_cmd.status.code(), Some(3), "{from_cmd:?}");
    assert!(
        !String::from_utf8_lossy(&from_cmd.stderr).contains("is not recognized"),
        "{from_cmd:?}",
    );
}

/// Ctrl+C at the console reaches every process attached to it. With the batch
/// context already gone, cmd.exe exits along with pnpm instead of asking about
/// a batch job.
#[test]
fn ctrl_c_ends_the_shim_without_asking_to_terminate_a_batch_job() {
    assert_interrupt_asks_nothing(Interrupt::CtrlC);
}

/// Ctrl+Break stops a script the way Ctrl+C does, and cmd.exe asks the same
/// question about a batch job it interrupts.
#[test]
fn ctrl_break_ends_the_shim_without_asking_to_terminate_a_batch_job() {
    assert_interrupt_asks_nothing(Interrupt::CtrlBreak);
}

#[derive(Debug, Clone, Copy)]
enum Interrupt {
    /// `CTRL_C_EVENT`, which reaches the whole console, as a keypress does.
    CtrlC,
    /// `CTRL_BREAK_EVENT`, sent to the process group of the `cmd.exe` the test
    /// starts.
    CtrlBreak,
}

/// Run `pnpm run dev` through `cmd /c` and the real `pnpm.cmd`, interrupt the
/// script once it is running, and assert that cmd.exe exits without asking
/// `Terminate batch job (Y/N)?`. With redirected stdio cmd.exe prints that
/// question on stdout and then waits for an answer from the console.
fn assert_interrupt_asks_nothing(interrupt: Interrupt) {
    let cli = GlobalCli::install();
    fs::write(
        cli.workspace.join("package.json"),
        json!({
            "name": "project",
            "version": "1.0.0",
            "scripts": {
                "dev": r#"node -e "require('fs').writeFileSync('started.txt', ''); setInterval(() => {}, 1000)""#,
            },
        })
        .to_string(),
    )
    .expect("write the project manifest");

    let _console = PrivateConsole::attach();
    let mut command = cli.via_cmd("run dev");
    if matches!(interrupt, Interrupt::CtrlBreak) {
        command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
    let mut child = command
        .with_stdin(Stdio::null())
        .with_stdout(Stdio::piped())
        .with_stderr(Stdio::piped())
        .spawn()
        .expect("spawn cmd");

    let started = cli.workspace.join("started.txt");
    let deadline = Instant::now() + Duration::from_mins(1);
    while !started.exists() {
        assert!(Instant::now() < deadline, "the script never started");
        assert!(
            child
                .try_wait()
                .expect("poll cmd")
                .is_none(),
            "cmd exited before the script ran",
        );
        sleep(Duration::from_millis(50));
    }

    let (event, group) = match interrupt {
        Interrupt::CtrlC => (CTRL_C_EVENT, 0),
        Interrupt::CtrlBreak => (CTRL_BREAK_EVENT, child.id()),
    };
    // SAFETY: plain FFI call with no pointer arguments. Group 0 is the whole
    // private console, where this process ignores the event (see
    // `PrivateConsole`); any other group is the one `CREATE_NEW_PROCESS_GROUP`
    // made for `child`.
    let sent = unsafe { GenerateConsoleCtrlEvent(event, group) };
    assert_ne!(sent, 0, "GenerateConsoleCtrlEvent: {}", std::io::Error::last_os_error());

    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll cmd") {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        sleep(Duration::from_millis(50));
    };
    if status.is_none() {
        // A prompt blocks on console input, so the process tree has to go.
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .status();
        let _ = child.wait();
    }
    let mut stdout = String::new();
    child.stdout
        .take()
        .expect("child stdout")
        .read_to_string(&mut stdout)
        .expect("read stdout");
    eprintln!("{interrupt:?}: status: {status:?}\nstdout: {stdout}");
    assert!(
        !stdout.contains("Terminate batch job"),
        "cmd.exe asked to terminate a batch job after {interrupt:?}:\n{stdout}",
    );
    assert!(status.is_some(), "cmd.exe kept running after {interrupt:?}");
}
