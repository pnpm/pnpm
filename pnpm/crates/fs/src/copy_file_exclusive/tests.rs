#[cfg(unix)]
use super::path_still_names;
use super::{copy_file_atomic, copy_file_exclusive};
use std::{fs, io};
use tempfile::TempDir;

#[test]
fn copies_the_bytes_into_a_new_file() {
    let dir = TempDir::new().unwrap();
    let (source, target) = (dir.path().join("source"), dir.path().join("target"));
    fs::write(&source, "content").unwrap();

    copy_file_exclusive(&source, &target, |_| Ok(())).unwrap();

    assert_eq!(fs::read_to_string(&target).unwrap(), "content");
}

#[test]
#[cfg(unix)]
fn carries_the_source_mode() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    let (source, target) = (dir.path().join("source"), dir.path().join("target"));
    fs::write(&source, "content").unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o750)).unwrap();

    copy_file_exclusive(&source, &target, |_| Ok(())).unwrap();

    assert_eq!(
        fs::metadata(&target)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o750,
    );
}

#[test]
#[cfg(unix)]
fn refuses_an_occupied_target_without_following_its_symlink() {
    let dir = TempDir::new().unwrap();
    let (source, target, victim) =
        (dir.path().join("source"), dir.path().join("target"), dir.path().join("victim"));
    fs::write(&source, "content").unwrap();
    fs::write(&victim, "untouched").unwrap();
    std::os::unix::fs::symlink(&victim, &target).unwrap();

    let error = copy_file_exclusive(&source, &target, |_| Ok(())).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(&victim).unwrap(), "untouched");
}

#[test]
fn removes_the_partial_file_when_finishing_fails() {
    let dir = TempDir::new().unwrap();
    let (source, target) = (dir.path().join("source"), dir.path().join("target"));
    fs::write(&source, "content").unwrap();

    let error = copy_file_exclusive(&source, &target, |_| Err(io::Error::other("finish failed")))
        .unwrap_err();

    assert_eq!(error.to_string(), "finish failed");
    assert!(!target.exists(), "the partial file is removed");
}

/// The failed-copy cleanup unlinks by path, so it has to confirm the
/// path still names what it created. A concurrent writer may rename a
/// complete file onto the target, and removing that would undo work
/// that already reported success.
///
/// Unix only: the Windows arm cannot stage this, since deleting a file
/// with an open handle leaves the name in place until the handle closes.
#[test]
#[cfg(unix)]
fn path_still_names_rejects_a_replaced_dirent() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("f");
    let created = fs::File::create(&path).unwrap();

    assert!(path_still_names(&created, &path), "the path names the file this call created");

    fs::remove_file(&path).unwrap();
    fs::write(&path, b"another importer's file").unwrap();

    assert!(!path_still_names(&created, &path), "a replaced dirent is not ours to remove");

    fs::remove_file(&path).unwrap();
    assert!(!path_still_names(&created, &path), "a path that names nothing has nothing to remove");
}

#[test]
fn copy_file_atomic_replaces_the_target_and_leaves_no_temp_file() {
    let dir = TempDir::new().unwrap();
    let (source, target) = (dir.path().join("source"), dir.path().join("target"));
    fs::write(&source, "new").unwrap();
    fs::write(&target, "old").unwrap();

    copy_file_atomic(&source, &target).unwrap();

    assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 2, "only the source and the target remain: {names:?}");
}

#[test]
#[cfg(unix)]
fn copy_file_atomic_replaces_a_symlink_without_following_it() {
    let dir = TempDir::new().unwrap();
    let (source, target, victim) =
        (dir.path().join("source"), dir.path().join("target"), dir.path().join("victim"));
    fs::write(&source, "content").unwrap();
    fs::write(&victim, "untouched").unwrap();
    std::os::unix::fs::symlink(&victim, &target).unwrap();

    copy_file_atomic(&source, &target).unwrap();

    let target_type = fs::symlink_metadata(&target).unwrap().file_type();
    assert!(!target_type.is_symlink(), "the symlink is replaced, not followed");
    assert_eq!(fs::read_to_string(&target).unwrap(), "content");
    assert_eq!(fs::read_to_string(&victim).unwrap(), "untouched");
}

#[test]
fn copy_file_atomic_leaves_no_temp_file_when_the_source_is_missing() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("target");

    let error = copy_file_atomic(&dir.path().join("missing"), &target).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0, "no temp file survives");
}
