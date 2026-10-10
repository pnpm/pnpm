use super::{LockMode, lock_file, try_lock_file};
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
    lock_file(&first, LockMode::Shared).unwrap();
    try_lock_file(&open(&path), LockMode::Shared).unwrap();

    assert!(matches!(
        try_lock_file(&open(&path), LockMode::Exclusive),
        Err(TryLockError::WouldBlock)
    ));

    drop(first);
    try_lock_file(&open(&path), LockMode::Exclusive).unwrap();
}

#[test]
fn an_exclusive_lock_blocks_every_other_handle() {
    let path = NamedTempFile::new().unwrap();
    let holder = open(&path);
    lock_file(&holder, LockMode::Exclusive).unwrap();

    assert!(matches!(
        try_lock_file(&open(&path), LockMode::Exclusive),
        Err(TryLockError::WouldBlock)
    ));
    assert!(matches!(try_lock_file(&open(&path), LockMode::Shared), Err(TryLockError::WouldBlock)));

    drop(holder);
    try_lock_file(&open(&path), LockMode::Exclusive).unwrap();
}
