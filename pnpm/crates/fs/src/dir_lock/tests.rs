use super::DirLock;
use std::{fs, time::Duration};
use tempfile::tempdir;

#[test]
fn acquire_creates_the_lock_and_drop_releases_it() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("nested").join("engine.lock");

    let lock = DirLock::acquire(path.clone(), Duration::ZERO, Duration::from_mins(1))
        .expect("acquire")
        .expect("uncontended lock is taken");
    assert!(path.is_dir(), "the lock directory exists while held");

    drop(lock);
    assert!(!path.exists(), "the lock directory is removed on drop");
}

#[test]
fn a_second_acquire_gives_up_while_the_first_is_held() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");

    let _held = DirLock::acquire(path.clone(), Duration::ZERO, Duration::from_mins(1))
        .expect("acquire")
        .expect("uncontended lock is taken");

    let contended = DirLock::acquire(path, Duration::from_millis(100), Duration::from_mins(1))
        .expect("acquire");
    assert!(contended.is_none(), "a held lock is not handed out twice");
}

/// A process that dies holding the lock must not wedge every later run,
/// so a lock older than the caller's timeout is taken over.
#[test]
fn an_abandoned_lock_is_taken_over() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");
    fs::create_dir(&path).expect("plant an abandoned lock");

    let taken = DirLock::acquire(path.clone(), Duration::ZERO, Duration::ZERO)
        .expect("acquire")
        .expect("an abandoned lock is taken over");

    assert!(path.is_dir());
    drop(taken);
    assert!(!path.exists());
}
