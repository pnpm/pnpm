//! Test-only worker invoked by the CAFS multi-process stress tests in
//! `tests/ensure_file_stress.rs`.
//!
//! Reads the content payload from `argv[1]` and the target CAS path
//! from `argv[2]`. An optional `--cas` selects the buffered CAS writer.
//! Exits `0` on success, `1` on error (with the error printed to
//! stderr).
//!
//! `cas_write_lock` is process-local, so the OS-level mutex tests need
//! real subprocesses to exercise the `O_CREAT | O_EXCL` +
//! `verify_or_rewrite` recovery path that holds the store together when
//! N processes race on the same blob.

#![expect(clippy::print_stderr, reason = "test-only helper binary")]

use pnpm_fs::ensure_file;
use std::{
    fs,
    path::PathBuf,
    process::ExitCode,
};

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

    let result = if args.next().as_deref() == Some("--cas") {
        pnpm_fs::ensure_cas_file(&target_path, &content, None).map(drop)
    } else {
        ensure_file(&target_path, &content, None)
    };
    if let Err(error) = result {
        eprintln!("{error}");
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}
