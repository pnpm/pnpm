//! Test-only worker invoked by the CAFS multi-process stress tests in
//! `tests/ensure_file_stress.rs`.
//!
//! Reads the content payload from `argv[1]` and the target CAS path
//! from `argv[2]`, then calls [`pacquet_fs::ensure_file`] exactly once.
//! Exits `0` on success, `1` on error (with the error printed to
//! stderr).
//!
//! Each worker is a separate process so the cross-process tests can
//! exercise real subprocesses. `cas_write_lock` is process-local, so
//! the OS-level mutex tests need real subprocesses to exercise the
//! `O_CREAT | O_EXCL` + `verify_or_rewrite` recovery path that holds the
//! store together when N processes race on the same blob.

use pacquet_fs::ensure_file;
use std::{fs, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(content_path) = args.next().map(PathBuf::from) else {
        eprintln!("usage: cafs_stress_worker <content-file> <target-file>");
        return ExitCode::from(2);
    };
    let Some(target_path) = args.next().map(PathBuf::from) else {
        eprintln!("usage: cafs_stress_worker <content-file> <target-file>");
        return ExitCode::from(2);
    };

    let content = match fs::read(&content_path) {
        Ok(content) => content,
        Err(error) => {
            eprintln!("failed to read content from {content_path:?}: {error}");
            return ExitCode::from(1);
        }
    };

    // `mode: None` lets `ensure_file` take the kernel default, which is
    // the same set of bits a `0o644` request would survive after a
    // typical umask. The test only inspects content, not mode, so the
    // simplification is safe.
    if let Err(error) = ensure_file(&target_path, &content, None) {
        eprintln!("ensure_file failed: {error}");
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}
