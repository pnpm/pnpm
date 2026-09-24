use super::{MAX_STORED_PATH, PendingTempFile, remove_pending_temp_files, track_temp_file};
use std::fs;

#[test]
fn pending_temp_file_is_unlinked_by_cleanup() {
    let dir = tempfile::tempdir().expect("tempdir");
    let temp = dir.path().join(".pnpm-lock.yaml.123.0.tmp");
    fs::write(&temp, "partial lockfile").expect("stage temp file");

    let _guard = track_temp_file(&temp);
    remove_pending_temp_files();

    assert!(!temp.exists(), "registered temp file should be unlinked: {temp:?}");
}

#[test]
fn released_temp_file_survives_cleanup() {
    let dir = tempfile::tempdir().expect("tempdir");
    let temp = dir.path().join(".pnpm-lock.yaml.123.1.tmp");
    fs::write(&temp, "published").expect("stage temp file");

    drop(track_temp_file(&temp));
    remove_pending_temp_files();

    assert!(temp.exists(), "a released temp file belongs to the writer again: {temp:?}");
}

#[test]
fn released_slot_is_reused() {
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
fn a_path_longer_than_the_slot_buffer_stays_untracked() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut long = dir.path().to_path_buf();
    while long.as_os_str().len() < MAX_STORED_PATH {
        long.push("x".repeat(200));
    }

    let guard: PendingTempFile = track_temp_file(&long);
    assert!(guard.entry.is_none(), "an over-long path gets no slot: {long:?}");
}
