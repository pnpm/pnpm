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
fn open_no_follow_opens_only_a_real_directory() {
    let root = tempdir().unwrap();
    create_tree(root.path());

    let real = OpenDir::open_no_follow(&root.path().join("real")).unwrap();
    let link = OpenDir::open_no_follow(&root.path().join("link")).unwrap();
    let file = OpenDir::open_no_follow(&root.path().join("real/file")).unwrap();
    let missing = OpenDir::open_no_follow(&root.path().join("missing")).unwrap();

    assert!(real.is_some());
    assert!(link.is_none());
    assert!(file.is_none());
    assert!(missing.is_none());
}

#[test]
fn remove_child_dir_removes_a_directory_with_its_contents() {
    let root = tempdir().unwrap();
    create_tree(root.path());
    let dir = OpenDir::open(&root.path().join("real")).unwrap();

    let removed = dir.remove_child_dir("metadata".as_ref()).unwrap();

    assert!(removed);
    let left: Vec<_> = fs::read_dir(root.path().join("real"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(left, ["file"]);
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
    let dir = OpenDir::open_no_follow(&root.path().join("real")).unwrap().unwrap();
    fs::rename(root.path().join("real"), root.path().join("moved")).unwrap();
    symlink_dir(&root.path().join("elsewhere"), &root.path().join("real")).unwrap();

    let canonical = dir.canonical_path().unwrap();
    let removed = dir.remove_child_dir("metadata".as_ref()).unwrap();

    assert_eq!(canonical, None);
    assert!(removed);
    assert!(
        root.path()
            .join("elsewhere/metadata/victim")
            .exists(),
    );
    assert!(
        !root
            .path()
            .join("moved/metadata")
            .exists(),
    );
    let left_behind: Vec<_> = fs::read_dir(root.path().join("moved"))
        .unwrap()
        .map(|entry| {
            entry
                .unwrap()
                .file_name()
                .into_string()
                .unwrap()
        })
        .filter(|name| name.starts_with("metadata."))
        .collect();
    assert_eq!(left_behind.len(), 1, "the renamed directory stays in the opened one");
}
