use super::{ModulesSwapLock, lock_path};
use std::fs;

#[test]
fn aliases_of_one_modules_dir_share_the_lock_path() {
    let temp_dir = tempfile::tempdir().unwrap();
    let real_root = temp_dir.path().join("real");
    let alias_root = temp_dir.path().join("alias");
    fs::create_dir(&real_root).unwrap();
    pnpm_fs::symlink_dir(&real_root, &alias_root).unwrap();

    let real = lock_path(&real_root.join("node_modules")).unwrap();
    let alias = lock_path(&alias_root.join("node_modules")).unwrap();
    let other = lock_path(&real_root.join("other_modules")).unwrap();

    assert_eq!(real, alias);
    assert_ne!(real, other);
}

#[test]
fn the_lock_is_taken_once_and_free_again_after_release() {
    let temp_dir = tempfile::tempdir().unwrap();
    let modules_dir = temp_dir.path().join("node_modules");

    let first = ModulesSwapLock::new(&modules_dir);
    assert!(first.hold());
    assert!(first.hold(), "a second hold reuses the lock the install already holds");
    first.release();

    let second = ModulesSwapLock::new(&modules_dir);
    assert!(second.hold(), "a released lock is free for the next install");
    drop(second);
    assert!(first.hold(), "a released lock can be taken again once it is free");
}

#[test]
fn an_install_that_could_not_take_the_lock_does_not_try_again() {
    let temp_dir = tempfile::tempdir().unwrap();
    // A file where the lock directory's parent must be created cannot be
    // taken, and the attempt fails at once.
    let modules_dir = temp_dir.path().join("node_modules");
    let lock = ModulesSwapLock::new(&modules_dir);
    *lock.state.lock().unwrap() = super::State::Unserialized;

    assert!(!lock.hold());
    lock.release();
    assert!(!lock.hold(), "a failed acquisition is not retried after a release either");
}
