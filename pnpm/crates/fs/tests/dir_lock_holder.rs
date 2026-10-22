//! Cross-process tests for [`pnpm_fs::DirLock`]: what a waiter sees of a
//! holder in another process, alive and killed.

use pnpm_fs::DirLock;
use std::{
    fs,
    io::{BufRead as _, BufReader},
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tempfile::tempdir;

/// Path to the test-only holder binary that `cargo build` produced
/// alongside this integration test.
const HOLDER_BIN: &str = env!("CARGO_BIN_EXE_dir_lock_holder");

/// Long enough that no lock in these tests is ever mistaken for
/// abandoned by age.
const NEVER_ABANDONED: Duration = Duration::from_mins(1);

/// Start a holder process and return once it holds the lock at `path`.
fn spawn_holder(path: &Path) -> Child {
    let mut holder = Command::new(HOLDER_BIN)
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn dir_lock_holder");
    let mut reply = String::new();
    BufReader::new(holder.stdout.take().expect("holder stdout"))
        .read_line(&mut reply)
        .expect("read the holder's reply");
    assert_eq!(reply.trim_end(), "held", "the holder took the lock");
    holder
}

/// A holder killed outright never runs its release, so its directory
/// stays behind. The next acquire must take it over at once rather than
/// wait for it to age
/// ([pnpm/pnpm#15360](https://github.com/pnpm/pnpm/issues/15360)).
#[test]
fn a_lock_left_by_a_killed_process_is_taken_over_at_once() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");
    let mut holder = spawn_holder(&path);

    holder.kill().expect("kill the holder");
    holder.wait().expect("reap the holder");
    assert!(path.is_dir(), "the killed holder left its lock directory behind");

    // Windows frees a killed process's file locks asynchronously, so the
    // waiter is given a moment; the takeover is still prompt next to
    // `NEVER_ABANDONED`.
    let taken = DirLock::acquire(path.clone(), Duration::from_secs(5), NEVER_ABANDONED)
        .expect("acquire")
        .expect("a killed holder's lock is taken over at once");
    assert!(taken.is_owner().expect("inspect owner"));

    drop(taken);
    assert!(!path.exists());
}

#[test]
fn a_lock_held_by_a_running_process_is_not_taken_over() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");
    let mut holder = spawn_holder(&path);

    let contended = DirLock::acquire(path.clone(), Duration::from_millis(100), NEVER_ABANDONED)
        .expect("acquire");
    assert!(contended.is_none(), "a running holder keeps its lock");

    drop(holder.stdin.take());
    let status = holder.wait().expect("reap the holder");
    assert!(status.success(), "the holder released the lock itself: {status}");
    assert!(!path.exists(), "the holder removed its lock directory");
    assert!(
        fs::read_dir(root.path())
            .expect("list the lock's parent")
            .next()
            .is_none(),
        "nothing of the released lock remains",
    );
    DirLock::acquire(path, Duration::ZERO, NEVER_ABANDONED)
        .expect("acquire")
        .expect("the released lock is free");
}
