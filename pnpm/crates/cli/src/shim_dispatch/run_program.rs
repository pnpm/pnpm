//! Running the program a dispatch resolved to: by replacing this process
//! with it, or, for a program that runs from a private install, in a
//! child this process waits for.

use pnpm_store_dir::PrivateInstall;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

/// Run `program` with `bin_dirs` prepended to `PATH`. A JavaScript
/// package manager needs the Node.js it was provisioned with to be
/// reachable, and its own directory has to come first so a nested
/// invocation finds the same version.
pub(super) fn exec_program_with_bin_dirs(
    program: &Path,
    bin_dirs: &[PathBuf],
    args: &[OsString],
) -> i32 {
    match program_command(program, args, bin_dirs) {
        Ok(command) => exec_command(program, command),
        Err(code) => code,
    }
}

/// Run `program` with `args`, replacing this process where the platform
/// allows. Exit codes follow the shell convention: 127 when the program
/// does not exist, 126 when it cannot be executed.
pub(super) fn exec_program(program: &Path, args: &[OsString]) -> i32 {
    exec_program_with_bin_dirs(program, &[], args)
}

/// Run `program` in a child and wait for it, for a program that runs
/// from the private installs in `held`: their in-use markers stay locked
/// until it exits, and are released before this process ends the way the
/// child did. An `exec` would drop the markers with the process image
/// and let a concurrent `pnpm store prune` remove the running copy.
pub(super) fn run_held_program(
    program: &Path,
    bin_dirs: &[PathBuf],
    args: &[OsString],
    held: Vec<PrivateInstall>,
) -> i32 {
    let mut command = match program_command(program, args, bin_dirs) {
        Ok(command) => command,
        Err(code) => return code,
    };
    let status = pnpm_executor::spawn_child(&mut command, None).and_then(|mut child| child.wait());
    drop(held);
    match status {
        Ok(status) => pnpm_executor::exit_like(pnpm_executor::ScriptExit::Process(status)),
        Err(error) => {
            eprintln!("pnpm: failed to run {}: {error}", program.display());
            if error.kind() == std::io::ErrorKind::NotFound { 127 } else { 126 }
        }
    }
}

/// The command running `program` with `args`, with `bin_dirs` prepended
/// to its `PATH`. The `PATH` travels on the command rather than through
/// this process's own environment: an `exec` hands the child the
/// command's environment just the same, and nothing here has to reason
/// about which threads are running. `Err` carries the exit code for a
/// `PATH` that cannot be built.
fn program_command(
    program: &Path,
    args: &[OsString],
    bin_dirs: &[PathBuf],
) -> Result<Command, i32> {
    let mut command = Command::new(program);
    command.args(args);
    if bin_dirs.is_empty() {
        return Ok(command);
    }
    match crate::path_env::prepend_dirs_to_path(bin_dirs) {
        Ok(path) => {
            crate::path_env::set_command_path(&mut command, path.as_os_str());
            Ok(command)
        }
        Err(error) => {
            // Rendered as a report so the failure carries the same
            // `ERR_PNPM_BAD_PATH_DIR` code the commands report it under.
            eprintln!("pnpm: {:?}", miette::Report::new(error));
            Err(1)
        }
    }
}

#[cfg(unix)]
fn exec_command(program: &Path, mut command: Command) -> i32 {
    use std::os::unix::process::CommandExt as _;
    let error = command.exec();
    eprintln!("pnpm: failed to exec {}: {error}", program.display());
    if error.kind() == std::io::ErrorKind::NotFound { 127 } else { 126 }
}

#[cfg(windows)]
fn exec_command(program: &Path, mut command: Command) -> i32 {
    // `.cmd`/`.bat` targets go to `Command::new` directly: the standard
    // library spawns them through `cmd.exe` itself with the
    // CVE-2024-24576 argument escaping, and rejects arguments it cannot
    // pass safely — a hand-rolled `cmd /c` would reintroduce that bug.
    match command.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("pnpm: failed to run {}: {error}", program.display());
            if error.kind() == std::io::ErrorKind::NotFound { 127 } else { 126 }
        }
    }
}
