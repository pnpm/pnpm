use super::{DirLock, HeldFile, Liveness};
use std::{
    fs::{self, File, TryLockError},
    io,
    path::Path,
    thread,
    thread::sleep,
    time::Duration,
};
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
    let held_path = super::held_path(&path).expect("resolve the held file");
    assert!(path.is_dir(), "the lock directory exists while held");
    assert!(held_path.is_file(), "the held file exists while held");

    drop(lock);
    assert!(!path.exists(), "the lock directory is removed on drop");
    assert!(!held_path.exists(), "the held file is removed on drop");
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
/// and nobody holding the file lock. The next acquire must not sit out
/// its wait for a holder that is not coming back
/// ([pnpm/pnpm#15360](https://github.com/pnpm/pnpm/issues/15360)).
#[test]
fn a_lock_whose_holder_died_is_taken_over_at_once() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");
    plant_dead_holders_lock(&path);

    let taken = DirLock::acquire(path.clone(), Duration::ZERO, NEVER_ABANDONED)
        .expect("acquire")
        .expect("a dead holder's lock is taken over");
    assert!(taken.is_owner().expect("inspect owner"));

    drop(taken);
    assert!(!path.exists(), "the successor releases it on its own drop");
}

/// Two waiters meeting the same dead holder must not both take it over:
/// the loser would remove the winner's directory and leave two holders.
#[test]
fn waiters_take_over_a_dead_holders_lock_one_at_a_time() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");
    plant_dead_holders_lock(&path);

    let waiters: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            thread::spawn(move || {
                let lock = DirLock::acquire(path, Duration::from_secs(5), NEVER_ABANDONED)
                    .expect("acquire")
                    .expect("each waiter gets its turn");
                sleep(Duration::from_millis(200));
                lock.is_owner().expect("inspect owner")
            })
        })
        .collect();

    for waiter in waiters {
        assert!(waiter.join().expect("waiter thread"), "a waiter kept the lock it took");
    }
    assert!(!path.exists());
}

/// A claimer that died right after creating the directory is told apart
/// from one still recording itself only by the directory's age, so a
/// fresh unrecorded directory is left alone.
#[test]
fn an_unrecorded_directory_is_left_to_its_claimer_while_fresh() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");
    fs::create_dir(&path).expect("plant a lock mid-claim");

    let contended =
        DirLock::acquire(path.clone(), Duration::ZERO, NEVER_ABANDONED).expect("acquire");

    assert!(contended.is_none(), "a claim in progress is not stolen");
    assert!(path.is_dir(), "the claimer's directory is left alone");
}

/// Whoever holds the file lock owns the directory or the right to claim
/// it, so a waiter that cannot get the file lock waits even when the
/// directory looks abandoned.
#[test]
fn a_directory_whose_held_file_is_locked_is_not_taken_over() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");
    fs::create_dir(&path).expect("plant an aged lock");
    let held = open_held_file(&path);
    crate::try_lock_file(&held, crate::LockMode::Exclusive).expect("hold the file lock");
    sleep(AGE);

    let contended = DirLock::acquire(path.clone(), Duration::ZERO, THRESHOLD).expect("acquire");

    assert!(contended.is_none(), "the file lock's holder keeps the directory");
    assert!(path.is_dir());
}

/// An older pnpm records its owner but keeps no file lock, so its
/// liveness cannot be told. Only the age threshold may declare such a
/// lock abandoned, or a mixed-version host runs the guarded work twice
/// at once.
#[test]
fn a_lock_without_a_held_marker_waits_for_the_age_threshold() {
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

/// A holder on another host proves its liveness through a held file of
/// its own, which this host's file lock says nothing about. Such a
/// directory is judged by age, not stolen from a running holder.
#[test]
fn a_lock_recorded_against_another_held_file_waits_for_the_age_threshold() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");
    fs::create_dir(&path).expect("plant another host's lock");
    fs::write(path.join(super::HELD_MARKER), "another-host").expect("plant its held marker");
    fs::write(path.join(super::OWNER_FILE), "1-2-3").expect("plant the owner record");

    let contended =
        DirLock::acquire(path.clone(), Duration::ZERO, NEVER_ABANDONED).expect("acquire");
    assert!(contended.is_none(), "a lock proven elsewhere is not stolen while fresh");

    sleep(AGE);
    DirLock::acquire(path, Duration::ZERO, THRESHOLD)
        .expect("acquire")
        .expect("an aged lock proven elsewhere is taken over");
}

#[test]
fn a_live_holder_keeps_the_held_file_locked() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");

    let held = DirLock::acquire(path.clone(), Duration::ZERO, NEVER_ABANDONED)
        .expect("acquire")
        .expect("uncontended lock is taken");
    let probe = open_held_file(&path);
    assert!(
        matches!(
            crate::try_lock_file(&probe, crate::LockMode::Exclusive),
            Err(TryLockError::WouldBlock)
        ),
        "the holder keeps the file lock",
    );

    drop(held);
    assert!(!path.exists());
    crate::try_lock_file(&probe, crate::LockMode::Exclusive)
        .expect("the file lock is released with the directory");
    // `probe` still holds an OS lock on the unlinked inode from the first
    // acquisition; a subsequent acquire must notice via `path_still_names`
    // that the unlinked inode no longer names `held_path` and lock a fresh
    // held file instead.
    let reacquired = DirLock::acquire(path.clone(), Duration::ZERO, NEVER_ABANDONED)
        .expect("reacquire")
        .expect("unlinked held inode does not block a new acquisition");
    drop(reacquired);
    assert!(!super::held_path(&path).expect("resolve held file").exists());
}

/// A holder that died without running `Drop` leaves its held file on disk
/// with the identity recorded in the lock directory, so every waiter that
/// locks it reads the same identity until the successor releases the lock.
#[test]
fn a_dead_holders_held_file_keeps_its_identity_until_taken_over() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");
    plant_dead_holders_lock(&path);
    let planted_id =
        fs::read_to_string(path.join(super::HELD_MARKER)).expect("read planted held marker");

    let mut waiter = HeldFile::open(&path);
    assert_eq!(waiter.lock(), Liveness::Proven);
    assert_eq!(waiter.id(), Some(planted_id.as_str()));
    drop(waiter);

    let taken = DirLock::acquire(path.clone(), Duration::ZERO, NEVER_ABANDONED)
        .expect("acquire")
        .expect("a dead holder's lock is taken over");
    drop(taken);
    assert!(!super::held_path(&path).expect("resolve held file").exists());
}

fn open_held_file(lock_path: &Path) -> File {
    let held_path = super::held_path(lock_path).expect("resolve the held file");
    pnpm_fs_open(&held_path)
}

fn pnpm_fs_open(path: &Path) -> File {
    crate::open_secure_lock_file(path).expect("open the held file")
}

fn plant_dead_holders_lock(path: &Path) {
    let mut held = HeldFile::open(path);
    assert_eq!(held.lock(), Liveness::Proven, "the held file is free to lock");
    let held_id = held.id().expect("a locked held file has an identity");
    fs::create_dir(path).expect("plant the lock directory");
    fs::write(path.join(super::HELD_MARKER), held_id).expect("plant the held marker");
    fs::write(path.join(super::OWNER_FILE), "1-2-3").expect("plant the owner record");
}

/// A holder judged abandoned by age — by an older pnpm, say — has lost
/// its lock to the next process in line. Releasing then must not remove
/// the successor's directory, or two processes hold the resource at once.
#[test]
fn a_holder_that_lost_its_lock_does_not_release_its_successors() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");

    let stale = DirLock::acquire(path.clone(), Duration::ZERO, NEVER_ABANDONED)
        .expect("acquire")
        .expect("uncontended lock is taken");
    fs::remove_dir_all(&path).expect("an older pnpm removes the directory it judged abandoned");
    fs::create_dir(&path).expect("and claims it");
    fs::write(path.join(super::OWNER_FILE), "1-2-3").expect("recording itself");

    assert!(!stale.is_owner().expect("inspect stale owner"));
    drop(stale);
    assert!(path.is_dir(), "the successor still holds the lock");
}

#[test]
fn claiming_a_directory_that_cannot_hold_the_record_fails() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");
    // A directory where the owner record belongs: the lock directory is
    // real, so the cleanup assertion below has something to observe, and
    // writing the record into it still fails.
    fs::create_dir_all(path.join("owner")).expect("block the owner record");

    let error = super::claim(path.clone(), HeldFile::open(&path))
        .expect_err("an unrecordable lock is not taken");

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

#[test]
fn persistent_held_file_open_error_falls_back_to_unavailable_after_retry_budget() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");
    let mut held = HeldFile::open(&path);
    let error = io::Error::from(io::ErrorKind::PermissionDenied);

    let initial = held.classify_open_error(&error);
    assert_eq!(initial, if cfg!(windows) { Liveness::Busy } else { Liveness::Unavailable });

    held.open_retry_started = Some(
        std::time::Instant::now()
            .checked_sub(super::RELEASE_RETRY_BUDGET + Duration::from_millis(1))
            .expect("instant before retry budget"),
    );
    assert_eq!(held.classify_open_error(&error), Liveness::Unavailable);
    assert!(held.held_path.is_none(), "held path is cleared once retry budget expires");
}

#[test]
fn missing_parent_held_file_open_retries_within_budget_then_falls_back() {
    let root = tempdir().expect("create tempdir");
    let path = root.path().join("engine.lock");
    let missing = root
        .path()
        .join("missing-dir")
        .join("lock.held");
    let mut held = HeldFile::open(&path);
    held.held_path = Some(missing.clone());

    assert!(
        held.open_file(&missing)
            .expect("within retry budget")
            .is_none(),
    );
    assert!(held.held_path.is_some(), "transient NotFound keeps held_path for retry");

    held.open_retry_started = Some(
        std::time::Instant::now()
            .checked_sub(super::RELEASE_RETRY_BUDGET + Duration::from_millis(1))
            .expect("instant before retry budget"),
    );
    assert_eq!(held.open_file(&missing).unwrap_err(), Liveness::Unavailable);
    assert!(held.held_path.is_none(), "expired retry budget clears held_path");
}
