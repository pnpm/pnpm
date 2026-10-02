use super::{lock_file, try_lock_file};
use std::fs::{File, TryLockError};
use tempfile::NamedTempFile;

fn open(path: &NamedTempFile) -> File {
    File::options()
        .read(true)
        .write(true)
        .open(path.path())
        .unwrap()
}

#[test]
fn shared_locks_overlap_and_block_an_exclusive_one() {
    let path = NamedTempFile::new().unwrap();
    let first = open(&path);
    lock_file(&first, false).unwrap();
    try_lock_file(&open(&path), false).unwrap();

    assert!(matches!(try_lock_file(&open(&path), true), Err(TryLockError::WouldBlock)));

    drop(first);
    try_lock_file(&open(&path), true).unwrap();
}

#[test]
fn an_exclusive_lock_blocks_every_other_handle() {
    let path = NamedTempFile::new().unwrap();
    let holder = open(&path);
    lock_file(&holder, true).unwrap();

    assert!(matches!(try_lock_file(&open(&path), true), Err(TryLockError::WouldBlock)));
    assert!(matches!(try_lock_file(&open(&path), false), Err(TryLockError::WouldBlock)));

    drop(holder);
    try_lock_file(&open(&path), true).unwrap();
}
