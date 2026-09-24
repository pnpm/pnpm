//! Test-only writer invoked by `tests/atomic_write_holder.rs`.
//!
//! The interrupt cleanup of a staged temp file can only be exercised from
//! another process: the signal handler and the pending-temp registry are
//! process-global, and the process must really die from the signal.

use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let Some(target) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: atomic_write_holder <target-file>");
        return ExitCode::from(2);
    };
    // Large enough that the write and the fsync underneath stay in flight
    // long enough for the test to interrupt them.
    let content = vec![b'x'; 256 * 1024 * 1024];
    match pnpm_fs::write_atomic(&target, &content) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("failed to write {}: {error}", target.display());
            ExitCode::from(1)
        }
    }
}
