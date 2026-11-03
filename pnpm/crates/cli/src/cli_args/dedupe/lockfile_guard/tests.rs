use super::LockfileGuard;
use std::fs;

const SNAPSHOT: &str = "lockfileVersion: '9.0'\n";
const CONFIG_DEPS_WRITE: &str = "lockfileVersion: '9.0'\nconfigDependencies: {}\n";
const EDITOR_WRITE: &str = "lockfileVersion: '9.0'\n# edited\n";

#[test]
fn a_settled_guard_keeps_an_edit_made_after_an_untouched_preparation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pnpm-lock.yaml");
    fs::write(&path, SNAPSHOT).unwrap();
    let mut guard = LockfileGuard::new(Some(SNAPSHOT.to_string()), &path);
    guard.settle();
    fs::write(&path, EDITOR_WRITE).unwrap();
    drop(guard);
    assert_eq!(fs::read_to_string(&path).unwrap(), EDITOR_WRITE);
}

#[test]
fn a_settled_guard_undoes_the_write_made_during_preparation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pnpm-lock.yaml");
    fs::write(&path, SNAPSHOT).unwrap();
    let mut guard = LockfileGuard::new(Some(SNAPSHOT.to_string()), &path);
    fs::write(&path, CONFIG_DEPS_WRITE).unwrap();
    guard.settle();
    drop(guard);
    assert_eq!(fs::read_to_string(&path).unwrap(), SNAPSHOT);
}

#[test]
fn a_settled_guard_keeps_an_edit_made_over_the_preparation_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pnpm-lock.yaml");
    fs::write(&path, SNAPSHOT).unwrap();
    let mut guard = LockfileGuard::new(Some(SNAPSHOT.to_string()), &path);
    fs::write(&path, CONFIG_DEPS_WRITE).unwrap();
    guard.settle();
    fs::write(&path, EDITOR_WRITE).unwrap();
    drop(guard);
    assert_eq!(fs::read_to_string(&path).unwrap(), EDITOR_WRITE);
}

#[test]
fn an_unsettled_guard_removes_a_lockfile_created_during_preparation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pnpm-lock.yaml");
    let guard = LockfileGuard::new(None, &path);
    fs::write(&path, CONFIG_DEPS_WRITE).unwrap();
    drop(guard);
    assert!(!path.exists());
}
