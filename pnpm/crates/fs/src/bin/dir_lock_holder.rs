//! Test-only holder invoked by `tests/dir_lock_holder.rs`.
//!
//! A separate process is the only way to exercise what the lock promises
//! about a holder that dies: the OS releases the file lock with the
//! process, which no thread inside the test can stand in for.

use pnpm_fs::DirLock;
use std::{
    io::{self, Read as _, Write as _},
    path::PathBuf,
    process::ExitCode,
    time::Duration,
};

fn main() -> ExitCode {
    let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: dir_lock_holder <lock-directory>");
        return ExitCode::from(2);
    };
    let lock = match DirLock::acquire(path, Duration::ZERO, Duration::from_mins(1)) {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            eprintln!("the lock is held by another process");
            return ExitCode::from(1);
        }
        Err(error) => {
            eprintln!("failed to take the lock: {error}");
            return ExitCode::from(1);
        }
    };
    println!("held");
    let _ = io::stdout().flush();
    let _ = io::stdin().read_to_end(&mut Vec::new());
    drop(lock);
    ExitCode::SUCCESS
}
