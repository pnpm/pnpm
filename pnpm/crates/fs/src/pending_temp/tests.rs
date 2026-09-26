use super::{ACTIVE_WALKS, remove_pending_temp_files, track_temp_file};
use std::{
    fs,
    sync::{Mutex, atomic::Ordering},
};

/// The registry is process-global and `cargo test` runs these tests on
/// threads of one process, so a cleanup in one test could unlink a temp
/// file another test is still holding registered. Serialize the tests that
/// touch the registry. (The signal-handler path itself must stay lock-free;
/// this is test-only serialization.)
static LOCK: Mutex<()> = Mutex::new(());

#[test]
fn pending_temp_file_is_unlinked_by_cleanup() {
    let _lock = LOCK.lock().expect("registry test lock");
    let dir = tempfile::tempdir().expect("tempdir");
    let temp = dir.path().join(".pnpm-lock.yaml.123.0.tmp");
    fs::write(&temp, "partial lockfile").expect("stage temp file");

    let _guard = track_temp_file(&temp);
    remove_pending_temp_files();

    assert!(!temp.exists(), "registered temp file should be unlinked: {temp:?}");
}

#[test]
fn released_temp_file_survives_cleanup() {
    let _lock = LOCK.lock().expect("registry test lock");
    let dir = tempfile::tempdir().expect("tempdir");
    let temp = dir.path().join(".pnpm-lock.yaml.123.1.tmp");
    fs::write(&temp, "published").expect("stage temp file");

    drop(track_temp_file(&temp));
    remove_pending_temp_files();

    assert!(temp.exists(), "a released temp file belongs to the writer again: {temp:?}");
}

#[test]
fn released_slot_is_reused() {
    let _lock = LOCK.lock().expect("registry test lock");
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.tmp");
    let second = dir.path().join("second.tmp");
    fs::write(&first, "1").expect("stage first temp file");
    fs::write(&second, "2").expect("stage second temp file");

    drop(track_temp_file(&first));
    let _guard = track_temp_file(&second);
    remove_pending_temp_files();

    assert!(first.exists(), "released slot must not be unlinked: {first:?}");
    assert!(!second.exists(), "reused slot must be unlinked: {second:?}");
}

#[test]
fn slot_released_during_a_walk_stays_usable() {
    let _lock = LOCK.lock().expect("registry test lock");
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.tmp");
    let second = dir.path().join("second.tmp");
    fs::write(&first, "1").expect("stage first temp file");
    fs::write(&second, "2").expect("stage second temp file");

    // A walk in progress keeps the released path alive rather than freeing
    // it under the walker; the slot itself is free for the next write.
    ACTIVE_WALKS.fetch_add(1, Ordering::SeqCst);
    drop(track_temp_file(&first));
    ACTIVE_WALKS.fetch_sub(1, Ordering::SeqCst);
    let _guard = track_temp_file(&second);
    remove_pending_temp_files();

    assert!(first.exists(), "released slot must not be unlinked: {first:?}");
    assert!(!second.exists(), "reused slot must be unlinked: {second:?}");
}
