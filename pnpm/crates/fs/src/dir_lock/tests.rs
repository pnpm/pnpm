use super::DirLock;
use std::{fs, thread::sleep, time::Duration};
use tempfile::tempdir;

/// How long the tests age a lock before declaring it abandoned, and the
/// threshold they declare it abandoned at.
///
/// A zero threshold would make the takeover tests depend on the host's
/// clocks agreeing: abandonment compares wall-clock now against the lock
/// directory's mtime, and on a runner whose wall clock sits ahead of the
/// filesystem's the subtraction underflows and the lock reads as fresh.
/// Aging the lock well past a non-zero threshold keeps the tests
/// deterministic on any host whose two clocks are within `AGE - THRESHOLD`
/// of each other.
const AGE: Duration = Duration::from_millis(150);
const THRESHOLD: Duration = Duration::from_millis(20);

/// Long enough that no lock in these tests is ever mistaken for
/// abandoned while it is deliberately held.
const NEVER_ABANDONED: Duration = Duration::from_mins(1);

#[test]
fn acquire_creates_the_lock_and_drop_releases_it() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("nested").join("engine.lock");

    let lock = DirLock::acquire(path.clone(), Duration::ZERO, NEVER_ABANDONED)
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

    let _held = DirLock::acquire(path.clone(), Duration::ZERO, NEVER_ABANDONED)
        .expect("acquire")
        .expect("uncontended lock is taken");

    let contended =
        DirLock::acquire(path, Duration::from_millis(100), NEVER_ABANDONED).expect("acquire");
    assert!(contended.is_none(), "a held lock is not handed out twice");
}

/// A process that dies holding the lock must not wedge every later run,
/// so a lock older than the caller's threshold is taken over.
#[test]
fn an_abandoned_lock_is_taken_over() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");
    fs::create_dir(&path).expect("plant an abandoned lock");
    sleep(AGE);

    let taken = DirLock::acquire(path.clone(), Duration::ZERO, THRESHOLD)
        .expect("acquire")
        .expect("an abandoned lock is taken over");

    assert!(path.is_dir());
    drop(taken);
    assert!(!path.exists());
}

/// A holder that outran the abandonment threshold has already lost its
/// lock to the next process in line. Releasing then must not remove the
/// successor's directory, or two processes hold the resource at once.
#[test]
fn a_stale_holder_does_not_release_its_successors_lock() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");

    let stale = DirLock::acquire(path.clone(), Duration::ZERO, NEVER_ABANDONED)
        .expect("acquire")
        .expect("uncontended lock is taken");
    sleep(AGE);
    let successor = DirLock::acquire(path.clone(), Duration::ZERO, THRESHOLD)
        .expect("acquire")
        .expect("an abandoned lock is taken over");

    drop(stale);
    assert!(path.is_dir(), "the successor still holds the lock");

    drop(successor);
    assert!(!path.exists(), "the successor releases it on its own drop");
}

#[test]
fn claiming_a_directory_that_cannot_hold_the_record_fails() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");
    // A directory where the owner record belongs: the lock directory is
    // real, so the cleanup assertion below has something to observe, and
    // writing the record into it still fails.
    fs::create_dir_all(path.join("owner")).expect("block the owner record");

    let error = super::claim(path.clone()).expect_err("an unrecordable lock is not taken");

    assert!(!path.exists(), "the lock directory is given back: {error}");
}
