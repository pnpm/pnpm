//! Cargo target runner for the Windows CI test job: runs the given program
//! with the default error mode instead of the one it would inherit.
//!
//! Node (libuv) sets `SEM_NOGPFAULTERRORBOX` at startup, and a child process
//! inherits its parent's error mode. The test job starts `cargo nextest`
//! under Node, so every test binary inherited that flag, and with it Windows
//! Error Reporting writes no crash dump when the process aborts. The runner
//! clears the error mode, so a crashing test binary leaves a dump
//! (pnpm/tasks#239).
//!
//! The program runs without the runner variable: `assert_cmd` would otherwise
//! start every binary a test spawns through this runner as well, which
//! changes the parent process those binaries see. They still inherit the
//! cleared error mode.
//!
//! Built with plain `rustc` by the workflow, so it is not a workspace member.

use std::process::{Command, exit};

const RUNNER_VAR: &str = "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER";

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetErrorMode(mode: u32) -> u32;
}

fn main() {
    // SAFETY: SetErrorMode only changes the error mode of this process, which
    // the child started below inherits.
    unsafe { SetErrorMode(0) };
    let mut args = std::env::args_os().skip(1);
    let Some(program) = args.next() else {
        eprintln!("usage: reset-error-mode <program> [args...]");
        exit(2);
    };
    let status = Command::new(&program)
        .args(args)
        .env_remove(RUNNER_VAR)
        .status()
        .unwrap_or_else(|error| {
            eprintln!("reset-error-mode: cannot run {}: {error}", program.to_string_lossy());
            exit(2);
        });
    // On Windows the exit code always exists: an abort reports 0xc0000409 as
    // a negative i32, which `exit` passes on unchanged.
    exit(status.code().unwrap_or(1));
}
