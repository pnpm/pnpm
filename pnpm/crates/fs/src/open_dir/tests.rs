//! Tests for the [`crate::OpenDir`] module.

use super::OpenDir;
use crate::symlink_dir;
use std::{fs, io, path::Path};
use tempfile::tempdir;

fn create_tree(root: &Path) {
    fs::create_dir_all(root.join("real/metadata/registry")).unwrap();
    fs::write(root.join("real/metadata/registry/pkg.json"), "{}").unwrap();
    fs::write(root.join("real/file"), "").unwrap();
    symlink_dir(&root.join("real"), &root.join("link")).unwrap();
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            entry
                .unwrap()
                .file_name()
                .into_string()
                .unwrap()
        })
        .collect();
    names.sort();
    names
}

/// Replace `root/real` with a link to `root/elsewhere`, moving the real
/// directory to `root/moved`.
#[cfg(unix)]
fn substitute_real(root: &Path) {
    fs::rename(root.join("real"), root.join("moved")).unwrap();
    symlink_dir(&root.join("elsewhere"), &root.join("real")).unwrap();
}

#[test]
fn open_follows_a_link_in_its_place() {
    let root = tempdir().unwrap();
    create_tree(root.path());

    let dir = OpenDir::open(&root.path().join("link")).expect("open through the link");

    assert_eq!(dir.path(), root.path().join("link"));
    assert_eq!(
        dir.canonical_path().unwrap(),
        Some(dunce::canonicalize(root.path().join("real")).unwrap()),
    );
}

#[test]
fn open_rejects_a_file_and_nothing() {
    let root = tempdir().unwrap();
    create_tree(root.path());

    let file = OpenDir::open(&root.path().join("real/file")).unwrap_err();
    let missing = OpenDir::open(&root.path().join("missing")).unwrap_err();

    assert_eq!(file.kind(), io::ErrorKind::NotADirectory);
    assert_eq!(missing.kind(), io::ErrorKind::NotFound);
}

#[test]
fn open_child_dir_opens_only_a_real_directory() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    let dir = OpenDir::open(root.path()).unwrap();

    let real = dir.open_child_dir("real".as_ref()).unwrap();
    let link = dir.open_child_dir("link".as_ref()).unwrap();
    let file = OpenDir::open(&root.path().join("real"))
        .unwrap()
        .open_child_dir("file".as_ref())
        .unwrap();
    let missing = dir.open_child_dir("missing".as_ref()).unwrap();

    assert_eq!(real.map(|dir| dir.path().to_path_buf()), Some(root.path().join("real")));
    assert!(link.is_none());
    assert!(file.is_none());
    assert!(missing.is_none());
}

#[test]
fn remove_child_dir_removes_a_directory_with_its_contents() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    fs::write(root.path().join("real/metadata/registry/other.json"), "{}").unwrap();
    symlink_dir(&root.path().join("real/file"), &root.path().join("real/metadata/to-file"))
        .unwrap();
    let dir = OpenDir::open(&root.path().join("real")).unwrap();

    let removed = dir.remove_child_dir("metadata".as_ref()).unwrap();

    assert!(removed);
    assert_eq!(names(&root.path().join("real")), ["file"]);
}

#[test]
fn remove_child_dir_keeps_a_file_a_link_and_reports_nothing() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    symlink_dir(&root.path().join("real/metadata"), &root.path().join("real/metadata-link"))
        .unwrap();
    let dir = OpenDir::open(&root.path().join("real")).unwrap();

    let file = dir.remove_child_dir("file".as_ref()).unwrap();
    let link = dir.remove_child_dir("metadata-link".as_ref()).unwrap();
    let missing = dir.remove_child_dir("missing".as_ref()).unwrap();

    assert!(!file);
    assert!(!link);
    assert!(!missing);
    assert!(root.path().join("real/file").exists());
    assert!(
        root.path()
            .join("real/metadata-link")
            .symlink_metadata()
            .unwrap()
            .is_symlink(),
    );
    assert!(
        root.path()
            .join("real/metadata/registry/pkg.json")
            .exists(),
    );
}

/// The race the handle closes: the directory is replaced by a link to
/// another directory after the open, and the removal must still act on
/// the opened directory, not on the link's target.
#[cfg(unix)]
#[test]
fn remove_child_dir_ignores_a_link_substituted_after_the_open() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    fs::create_dir_all(root.path().join("elsewhere/metadata")).unwrap();
    fs::write(root.path().join("elsewhere/metadata/victim"), "").unwrap();
    let dir = OpenDir::open(&root.path().join("real")).unwrap();
    substitute_real(root.path());

    let canonical = dir.canonical_path().unwrap();
    let removed = dir.remove_child_dir("metadata".as_ref()).unwrap();

    assert_eq!(canonical, None);
    assert!(removed);
    assert!(
        root.path()
            .join("elsewhere/metadata/victim")
            .exists(),
    );
    assert_eq!(names(&root.path().join("moved")), ["file"]);
}

/// Same race one level up: the parent is replaced after the open, and a
/// child opened through it must be the opened parent's child.
#[cfg(unix)]
#[test]
fn open_child_dir_ignores_a_link_substituted_after_the_open() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    fs::create_dir_all(root.path().join("real/v12/metadata")).unwrap();
    fs::create_dir_all(root.path().join("elsewhere/v12/metadata")).unwrap();
    fs::write(root.path().join("elsewhere/v12/metadata/victim"), "").unwrap();
    let dir = OpenDir::open(&root.path().join("real")).unwrap();
    substitute_real(root.path());

    let child = dir
        .open_child_dir("v12".as_ref())
        .unwrap()
        .expect("the opened directory still has v12");
    let canonical = child.canonical_path().unwrap();
    let removed = child.remove_child_dir("metadata".as_ref()).unwrap();

    assert_eq!(canonical, None);
    assert!(removed);
    assert!(
        root.path()
            .join("elsewhere/v12/metadata/victim")
            .exists(),
    );
    assert_eq!(names(&root.path().join("moved/v12")), Vec::<String>::new());
}
