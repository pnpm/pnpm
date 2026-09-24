//! Cross-process test for the interrupt cleanup of the temp file that
//! [`pnpm_fs::write_atomic`] stages next to its target
//! ([pnpm/pnpm#1418](https://github.com/pnpm/pnpm/issues/1418)). The writer
//! must really die from the signal, which no thread inside the test can
//! stand in for. Windows has no SIGINT delivery to a child; `kill` there is
//! `TerminateProcess`, against which no cleanup can run.

#![cfg(unix)]

use std::{
    fs,
    os::unix::process::ExitStatusExt as _,
    path::Path,
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};
use tempfile::tempdir;

/// Path to the test-only writer binary that `cargo build` produced
/// alongside this integration test.
const HOLDER_BIN: &str = env!("CARGO_BIN_EXE_atomic_write_holder");

#[test]
fn a_sigint_mid_write_removes_the_staged_temp_file() {
    let root = tempdir().expect("create tempdir");
    let target = root.path().join("lock.yaml");
    let mut writer = Command::new(HOLDER_BIN)
        .arg(&target)
        .spawn()
        .expect("spawn atomic_write_holder");

    wait_for_temp_file(root.path(), &mut writer);
    // SAFETY: the writer is this test's own child and still running.
    unsafe {
        libc::kill(i32::try_from(writer.id()).expect("the child's pid fits in i32"), libc::SIGINT);
    }
    let status = writer.wait().expect("reap the writer");
    assert_eq!(status.signal(), Some(libc::SIGINT), "the writer died from SIGINT");

    let leftovers: Vec<_> = fs::read_dir(root.path())
        .expect("list the project directory")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .filter(|name| name.to_string_lossy().starts_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "the interrupt left temp files behind: {leftovers:?}");
}

/// Poll until the writer's staged temp file shows up. Interrupting earlier
/// would pass without exercising the cleanup, as there would be no temp
/// file to remove.
fn wait_for_temp_file(dir: &Path, writer: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let staged = fs::read_dir(dir)
            .expect("list the project directory")
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".tmp")
            });
        if staged {
            return;
        }
        assert!(
            writer
                .try_wait()
                .expect("poll the writer")
                .is_none(),
            "the writer exited before staging a temp file"
        );
        assert!(Instant::now() < deadline, "the writer staged no temp file in 30s");
        thread::sleep(Duration::from_millis(2));
    }
}
