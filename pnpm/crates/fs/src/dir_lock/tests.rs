use super::DirLock;
use std::{fs, io, thread::sleep, time::Duration};
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
    let path = root
        .path()
        .join("nested")
        .join("engine.lock");

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

/// A process killed while holding the lock leaves its directory behind
/// with nobody holding the OS lock on the held file. The next acquire
/// must not sit out its wait for a holder that is not coming back
/// ([pnpm/pnpm#15360](https://github.com/pnpm/pnpm/issues/15360)).
#[test]
fn a_lock_whose_holder_died_is_taken_over_at_once() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");
    plant_dead_holders_lock(&path);

    let started = std::time::Instant::now();
    let taken = DirLock::acquire(path.clone(), Duration::ZERO, NEVER_ABANDONED)
        .expect("acquire")
        .expect("a dead holder's lock is taken over");
    assert!(
        started.elapsed() < NEVER_ABANDONED,
        "the takeover does not wait for the age threshold",
    );
    assert!(taken.is_owner().expect("inspect owner"));

    drop(taken);
    assert!(!path.exists(), "the successor releases it on its own drop");
}

/// A holder that took the directory but has not recorded itself yet is
/// mid-claim, not dead, even though its held file is still unlocked.
#[test]
fn a_claim_in_progress_is_not_taken_over() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");
    fs::create_dir(&path).expect("plant a lock mid-claim");
    fs::write(path.join(super::HELD_FILE), "").expect("plant the held file");

    let contended =
        DirLock::acquire(path.clone(), Duration::ZERO, NEVER_ABANDONED).expect("acquire");

    assert!(contended.is_none(), "a claim in progress is not stolen");
    assert!(path.is_dir(), "the claimer's directory is left alone");
}

/// An older pnpm records its owner but keeps no OS lock, so its liveness
/// cannot be told. Only the age threshold may declare such a lock
/// abandoned, or a mixed-version host runs the guarded work twice at
/// once.
#[test]
fn a_lock_without_a_held_file_waits_for_the_age_threshold() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");
    fs::create_dir(&path).expect("plant an older pnpm's lock");
    fs::write(path.join(super::OWNER_FILE), "1-2-3").expect("plant the owner record");

    let contended =
        DirLock::acquire(path.clone(), Duration::ZERO, NEVER_ABANDONED).expect("acquire");
    assert!(contended.is_none(), "a lock of unknown liveness is not stolen while fresh");

    sleep(AGE);
    let taken = DirLock::acquire(path, Duration::ZERO, THRESHOLD)
        .expect("acquire")
        .expect("an aged lock of unknown liveness is taken over");
    assert!(taken.is_owner().expect("inspect owner"));
}

/// The holder keeps the OS lock for as long as it holds the directory,
/// so a live holder never reads as dead to a waiter.
#[test]
fn a_live_holder_keeps_its_held_file_locked() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");

    let held = DirLock::acquire(path.clone(), Duration::ZERO, NEVER_ABANDONED)
        .expect("acquire")
        .expect("uncontended lock is taken");
    assert!(!super::holder_is_gone(&path), "a live holder is not reported gone");

    drop(held);
    assert!(!path.exists());
}

/// Plant what a process killed while holding the lock leaves behind: the
/// directory, its owner record, and a held file nobody has locked.
fn plant_dead_holders_lock(path: &std::path::Path) {
    fs::create_dir(path).expect("plant the lock directory");
    fs::write(path.join(super::HELD_FILE), "").expect("plant the held file");
    fs::write(path.join(super::OWNER_FILE), "1-2-3").expect("plant the owner record");
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

    assert!(!stale.is_owner().expect("inspect stale owner"));
    assert!(successor.is_owner().expect("inspect successor owner"));
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

#[test]
fn transient_release_error_classifier_is_windows_specific() {
    for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::ResourceBusy] {
        let error = io::Error::from(kind);
        assert_eq!(super::is_transient_release_error(&error), cfg!(windows), "{kind:?}");
    }

    for kind in [io::ErrorKind::NotFound, io::ErrorKind::InvalidInput, io::ErrorKind::Other] {
        assert!(!super::is_transient_release_error(&io::Error::from(kind)), "{kind:?}");
    }
}
