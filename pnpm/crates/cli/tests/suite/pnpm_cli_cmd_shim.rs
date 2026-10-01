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

use assert_cmd::cargo::CommandCargoExt;
use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, command_env::CommandTestExt};
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Command, Output, Stdio},
    thread::sleep,
    time::{Duration, Instant},
};
use tempfile::TempDir;
use windows_sys::Win32::System::{
    Console::{AllocConsole, CTRL_BREAK_EVENT, FreeConsole, GenerateConsoleCtrlEvent},
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
        fs::create_dir_all(&package_dir).expect("create the CLI package dir");
        fs::copy(cargo_bin_pnpm(), package_dir.join("pnpm.exe")).expect("copy pnpm.exe");
        fs::write(
            package_dir.join("package.json"),
            json!({
                "name": "pnpm",
                "version": "12.0.0",
                "bin": { "pnpm": "pnpm.exe", "pn": "pnpm.exe" },
                "files": ["pnpm.exe"],
            })
            .to_string(),
        )
        .expect("write the CLI package manifest");
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
                    "exit-3": "node -e \"process.exit(3)\"",
                    "exit-0": "node -e \"\"",
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
        command.raw_arg(format!("/d /c \"\"{}\" {command_line}\"", self.shim().display()));
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
    format!("\"{arg}\"")
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
        command.raw_arg(format!("/d /c \"{}\"", driver.display()));
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
/// survive that unchanged, which they did before the shim ended its batch
/// context too.
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
    let pipe_through = "exec node -e \"process.stdin.pipe(process.stdout)\"";

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

/// Ctrl+Break stops a script the way Ctrl+C does, and cmd.exe asks the same
/// question about a batch job it interrupts. With the batch context already
/// gone, cmd.exe exits along with pnpm instead.
#[test]
fn ctrl_break_ends_the_shim_without_asking_to_terminate_a_batch_job() {
    let cli = GlobalCli::install();
    fs::write(
        cli.workspace.join("package.json"),
        json!({
            "name": "project",
            "version": "1.0.0",
            "scripts": {
                "dev": "node -e \"require('fs').writeFileSync('started.txt', ''); setInterval(() => {}, 1000)\"",
            },
        })
        .to_string(),
    )
    .expect("write the project manifest");

    let _console = PrivateConsole::attach();
    let mut command = cli.via_cmd("run dev");
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
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
            "cmd exited before the script ran"
        );
        sleep(Duration::from_millis(50));
    }

    // SAFETY: plain FFI call with no pointer arguments. The group is the one
    // `CREATE_NEW_PROCESS_GROUP` made for `child`, so the event reaches cmd.exe,
    // pnpm, and the script, and nothing else on the console.
    let sent = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, child.id()) };
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
    eprintln!("status: {status:?}\nstdout: {stdout}");
    assert!(
        !stdout.contains("Terminate batch job"),
        "cmd.exe asked to terminate a batch job:\n{stdout}",
    );
    assert!(status.is_some(), "cmd.exe kept running after Ctrl+Break");
}

/// A console of this test process's own, so the batch job prompt and the
/// control event stay away from whatever console runs the test suite. Each
/// test runs in its own process under nextest, so the swap affects no other
/// test.
struct PrivateConsole;

impl PrivateConsole {
    fn attach() -> Self {
        // SAFETY: plain FFI calls with no arguments. Detaching fails harmlessly
        // when the process has no console to begin with.
        let allocated = unsafe {
            FreeConsole();
            AllocConsole()
        };
        assert_ne!(allocated, 0, "AllocConsole: {}", std::io::Error::last_os_error());
        PrivateConsole
    }
}

impl Drop for PrivateConsole {
    fn drop(&mut self) {
        // SAFETY: plain FFI call with no arguments.
        unsafe {
            FreeConsole();
        }
    }
}
