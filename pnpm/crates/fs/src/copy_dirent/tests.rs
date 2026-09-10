use crate::copy_dirent::{copy_dir_contents, copy_dirent};
use std::fs;
use tempfile::tempdir;

#[cfg(unix)]
use std::os::unix::fs::symlink;

#[test]
fn a_nested_tree_is_copied_whole() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(src.join("deep/deeper")).unwrap();
    fs::write(src.join("top.txt"), b"top").unwrap();
    fs::write(src.join("deep/deeper/leaf.txt"), b"leaf").unwrap();

    let dst = tmp.path().join("dst");
    copy_dirent(&src, &dst).unwrap();

    assert_eq!(fs::read(dst.join("top.txt")).unwrap(), b"top");
    assert_eq!(fs::read(dst.join("deep/deeper/leaf.txt")).unwrap(), b"leaf");
    assert!(src.join("top.txt").exists(), "the copy must leave the source in place");
}

#[test]
fn an_empty_directory_is_copied_as_a_directory() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir(&src).unwrap();

    let dst = tmp.path().join("dst");
    copy_dirent(&src, &dst).unwrap();

    assert!(dst.is_dir());
}

#[test]
fn a_single_file_is_copied() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("file.txt");
    fs::write(&src, b"contents").unwrap();

    let dst = tmp.path().join("copy.txt");
    copy_dirent(&src, &dst).unwrap();

    assert_eq!(fs::read(&dst).unwrap(), b"contents");
}

/// A `node_modules/` is mostly symlinks into the virtual store.
/// Following one would replace the link with a second copy of the
/// package, so the copy has to recreate links as links.
#[cfg(unix)]
#[test]
fn symlinks_are_recreated_rather_than_followed() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(src.join("real")).unwrap();
    fs::write(src.join("real/index.js"), b"// dep").unwrap();
    symlink("real", src.join("link-to-dir")).unwrap();
    symlink("nowhere", src.join("dangling")).unwrap();

    let dst = tmp.path().join("dst");
    copy_dirent(&src, &dst).unwrap();

    assert_eq!(fs::read_link(dst.join("link-to-dir")).unwrap().as_os_str(), "real");
    assert_eq!(fs::read_link(dst.join("dangling")).unwrap().as_os_str(), "nowhere");
    assert!(
        !dst.join("link-to-dir").symlink_metadata().unwrap().is_dir(),
        "the link must not have been resolved into a directory of its own",
    );
}

#[test]
fn copy_dir_contents_merges_into_an_existing_directory() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("added.txt"), b"added").unwrap();

    let dst = tmp.path().join("dst");
    fs::create_dir(&dst).unwrap();
    fs::write(dst.join("kept.txt"), b"kept").unwrap();

    copy_dir_contents(&src, &dst).unwrap();

    assert_eq!(fs::read(dst.join("kept.txt")).unwrap(), b"kept");
    assert_eq!(fs::read(dst.join("added.txt")).unwrap(), b"added");
}

#[test]
fn a_missing_source_reports_not_found() {
    let tmp = tempdir().unwrap();
    let error = copy_dirent(&tmp.path().join("absent"), &tmp.path().join("dst")).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}
