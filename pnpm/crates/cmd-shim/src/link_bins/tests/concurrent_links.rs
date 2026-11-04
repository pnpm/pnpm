use super::{
    super::executable::symlink_tolerating_concurrent_writers, Path, create_dir_all, tempdir,
    write_file,
};
use std::{os::unix::fs::symlink, path::PathBuf};
use tempfile::TempDir;

fn create_concurrent_link_fixture() -> (TempDir, PathBuf, PathBuf) {
    let tmp = tempdir().unwrap();
    create_dir_all(tmp.path().join(".bin")).unwrap();
    let target = tmp.path().join("package/cli.js");
    create_dir_all(target.parent().unwrap()).unwrap();
    write_file(&target, "cli").unwrap();
    let shim = tmp.path().join(".bin/cli");
    (tmp, target, shim)
}

fn already_exists() -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
}

#[test]
fn concurrent_executable_symlink_creation_accepts_the_expected_target() {
    let (_tmp, target, shim) = create_concurrent_link_fixture();
    symlink("../package/cli.js", &shim).unwrap();

    symlink_tolerating_concurrent_writers(&shim, &target, None, already_exists).unwrap();

    assert_eq!(std::fs::read_link(shim).unwrap(), Path::new("../package/cli.js"));
}

#[test]
fn concurrent_executable_symlink_creation_rejects_a_different_target() {
    let (_tmp, target, shim) = create_concurrent_link_fixture();
    symlink("../package/other.js", &shim).unwrap();

    let error =
        symlink_tolerating_concurrent_writers(&shim, &target, None, already_exists).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
}

/// A third writer removes the link between this writer's conflict and its
/// check, so the creation must be retried rather than reported.
#[test]
fn concurrent_executable_symlink_creation_retries_after_the_link_is_removed() {
    let (_tmp, target, shim) = create_concurrent_link_fixture();
    let mut attempts = 0;

    symlink_tolerating_concurrent_writers(&shim, &target, None, || {
        attempts += 1;
        if attempts == 1 { already_exists() } else { symlink("../package/cli.js", &shim) }
    })
    .unwrap();

    assert_eq!(attempts, 2);
    assert_eq!(std::fs::read_link(shim).unwrap(), Path::new("../package/cli.js"));
}

#[test]
fn concurrent_executable_symlink_creation_stops_retrying() {
    let (_tmp, target, shim) = create_concurrent_link_fixture();
    let mut attempts = 0;

    let error = symlink_tolerating_concurrent_writers(&shim, &target, None, || {
        attempts += 1;
        already_exists()
    })
    .unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(attempts, 4);
}

/// An absolute node link inside the relocatable root stops working when the
/// root moves, so a concurrent writer's absolute link is not accepted there.
#[test]
fn concurrent_node_symlink_creation_rejects_an_absolute_link_in_a_relocatable_root() {
    let tmp = tempdir().unwrap();
    create_dir_all(tmp.path().join(".bin")).unwrap();
    let target = tmp.path().join("runtime/bin/node");
    create_dir_all(target.parent().unwrap()).unwrap();
    write_file(&target, "node").unwrap();
    let shim = tmp.path().join(".bin/node");
    symlink(&target, &shim).unwrap();

    let error =
        symlink_tolerating_concurrent_writers(&shim, &target, Some(tmp.path()), already_exists)
            .unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
}
