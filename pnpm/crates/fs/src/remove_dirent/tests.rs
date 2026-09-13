//! Tests for the [`crate::remove_dirent`] module.

use super::remove_dirent;
use crate::symlink_dir;
use std::{fs, io, path::Path};
use tempfile::tempdir;

/// Assert `path` no longer exists, distinguishing "gone" (`NotFound`)
/// from an inspection failure that would leave the entry in place.
#[track_caller]
fn assert_gone(path: &Path, what: &str) {
    let metadata = fs::symlink_metadata(path);
    assert!(
        matches!(&metadata, Err(error) if error.kind() == io::ErrorKind::NotFound),
        "{what} must be gone, got {metadata:?}",
    );
}

#[test]
fn removes_a_regular_file() {
    let root = tempdir().expect("create temp dir");
    let file = root.path().join("file.txt");
    fs::write(&file, "contents").expect("write file");

    remove_dirent(&file).expect("remove the file");

    assert!(!file.exists());
}

#[test]
fn removes_a_directory_tree() {
    let root = tempdir().expect("create temp dir");
    let dir = root.path().join("dir");
    fs::create_dir_all(dir.join("nested")).expect("create nested dirs");
    fs::write(dir.join("nested").join("file.txt"), "contents").expect("write file");

    remove_dirent(&dir).expect("remove the directory tree");

    assert!(!dir.exists());
}

#[test]
fn removes_a_directory_link_without_touching_its_target() {
    let root = tempdir().expect("create temp dir");
    let target = root.path().join("target");
    let link = root.path().join("link");
    fs::create_dir(&target).expect("create target dir");
    fs::write(target.join("file.txt"), "contents").expect("write file in target");
    symlink_dir(&target, &link).expect("create link");

    remove_dirent(&link).expect("remove the link");

    assert_gone(&link, "the link itself");
    assert!(target.join("file.txt").exists(), "the target must be untouched");
}

/// The shape `pnpm clean` hits after removing `node_modules/.pnpm`
/// before the top-level package links
/// ([pnpm/pnpm#13694](https://github.com/pnpm/pnpm/issues/13694)): the
/// link dangles, so any dispatch that follows it sees a non-directory.
#[test]
fn removes_a_dangling_directory_link() {
    let root = tempdir().expect("create temp dir");
    let target = root.path().join("target");
    let link = root.path().join("link");
    fs::create_dir(&target).expect("create target dir");
    symlink_dir(&target, &link).expect("create link");
    fs::remove_dir(&target).expect("remove target to dangle the link");

    remove_dirent(&link).expect("remove the dangling link");

    assert_gone(&link, "the dangling link");
}

#[cfg(unix)]
#[test]
fn removes_a_file_symlink() {
    let root = tempdir().expect("create temp dir");
    let target = root.path().join("file.txt");
    let link = root.path().join("link");
    fs::write(&target, "contents").expect("write target file");
    std::os::unix::fs::symlink(&target, &link).expect("create file symlink");

    remove_dirent(&link).expect("remove the file symlink");

    assert_gone(&link, "the link");
    assert!(target.exists(), "the target must be untouched");
}

#[cfg(windows)]
#[test]
fn windows_removes_a_junction_without_touching_its_target() {
    let root = tempdir().expect("create temp dir");
    let target = root.path().join("target");
    let link = root.path().join("link");
    fs::create_dir(&target).expect("create target dir");
    fs::write(target.join("file.txt"), "contents").expect("write file in target");
    junction::create(&target, &link).expect("create junction");

    remove_dirent(&link).expect("remove the junction");

    assert_gone(&link, "the junction itself");
    assert!(target.join("file.txt").exists(), "the target must be untouched");
}

#[cfg(windows)]
#[test]
fn windows_removes_a_dangling_junction() {
    let root = tempdir().expect("create temp dir");
    let target = root.path().join("target");
    let link = root.path().join("link");
    fs::create_dir(&target).expect("create target dir");
    junction::create(&target, &link).expect("create junction");
    fs::remove_dir(&target).expect("remove target to dangle the junction");

    remove_dirent(&link).expect("remove the dangling junction");

    assert_gone(&link, "the dangling junction");
}
