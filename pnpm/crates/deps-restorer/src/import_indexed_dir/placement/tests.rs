use super::{
    FsRemoveDirAll, FsRemoveNonDirDirent, clear_dir_blocking_file, clear_dirent_blocking_dir,
    dir_fits_at, entries_by_target_dir, file_fits_at,
};
use std::{
    collections::{BTreeSet, HashMap},
    fs, io,
    path::{Path, PathBuf},
};
use tempfile::tempdir;

#[test]
fn entries_are_grouped_by_the_directory_they_land_in() {
    let cas_paths: HashMap<String, PathBuf> =
        ["package.json", "README.md", "lib/index.js", "lib/util.js", "lib/esm/index.js"]
            .into_iter()
            .map(|entry| (entry.to_string(), PathBuf::from("store").join(entry)))
            .collect();

    let groups: BTreeSet<BTreeSet<&str>> = entries_by_target_dir(&cas_paths, Some("package.json"))
        .iter()
        .map(|entries| {
            entries
                .iter()
                .map(|(entry, store_path)| {
                    assert_eq!(*store_path, cas_paths[*entry]);
                    *entry
                })
                .collect()
        })
        .collect();

    assert_eq!(
        groups,
        BTreeSet::from([
            BTreeSet::from(["README.md"]),
            BTreeSet::from(["lib/index.js", "lib/util.js"]),
            BTreeSet::from(["lib/esm/index.js"]),
        ]),
    );
}

#[test]
fn nothing_at_a_path_fits_either_kind() {
    let root = tempdir().unwrap();
    let absent = root.path().join("absent");

    assert!(dir_fits_at(&absent));
    assert!(file_fits_at(&absent));
}

#[test]
fn a_directory_fits_only_where_a_directory_belongs() {
    let root = tempdir().unwrap();
    let target = root.path().join("target");
    fs::create_dir(&target).unwrap();

    assert!(dir_fits_at(&target));
    assert!(!file_fits_at(&target));
}

#[test]
fn a_file_fits_only_where_a_file_belongs() {
    let root = tempdir().unwrap();
    let target = root.path().join("target");
    fs::write(&target, b"payload").unwrap();

    assert!(file_fits_at(&target));
    assert!(!dir_fits_at(&target));
}

// Each fake stands in for the installer sharing the slot, which can finish
// the same work between this one's inspection and its removal. The removal
// always fails, because that is what the loser of the race sees; what the
// path holds by then is what the fakes vary.
struct ReplacedByWhatBelongs;
struct RemovedByTheOtherInstaller;
struct LeavesTheBlocker;

impl FsRemoveDirAll for ReplacedByWhatBelongs {
    fn remove_dir_all(path: &Path) -> io::Result<()> {
        fs::remove_dir_all(path)?;
        fs::write(path, b"payload")?;
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

impl FsRemoveDirAll for RemovedByTheOtherInstaller {
    fn remove_dir_all(path: &Path) -> io::Result<()> {
        fs::remove_dir_all(path)?;
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

impl FsRemoveDirAll for LeavesTheBlocker {
    fn remove_dir_all(_: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

impl FsRemoveNonDirDirent for ReplacedByWhatBelongs {
    fn remove_non_dir_dirent(path: &Path, _: fs::FileType) -> io::Result<()> {
        fs::remove_file(path)?;
        fs::create_dir(path)?;
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

impl FsRemoveNonDirDirent for RemovedByTheOtherInstaller {
    fn remove_non_dir_dirent(path: &Path, _: fs::FileType) -> io::Result<()> {
        fs::remove_file(path)?;
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

impl FsRemoveNonDirDirent for LeavesTheBlocker {
    fn remove_non_dir_dirent(_: &Path, _: fs::FileType) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

#[test]
fn a_directory_replaced_by_the_file_counts_as_cleared() {
    let root = tempdir().unwrap();
    let target = root.path().join("target");
    fs::create_dir(&target).unwrap();

    clear_dir_blocking_file::<ReplacedByWhatBelongs>(&target)
        .expect("the path holds the file the clearing was for");
}

#[test]
fn a_directory_removed_by_the_other_installer_counts_as_cleared() {
    let root = tempdir().unwrap();
    let target = root.path().join("target");
    fs::create_dir(&target).unwrap();

    clear_dir_blocking_file::<RemovedByTheOtherInstaller>(&target)
        .expect("nothing stands where the file belongs");
}

#[test]
fn a_directory_still_standing_fails_the_clearing() {
    let root = tempdir().unwrap();
    let target = root.path().join("target");
    fs::create_dir(&target).unwrap();

    clear_dir_blocking_file::<LeavesTheBlocker>(&target)
        .expect_err("the blocker is still in the way");
}

#[test]
fn a_file_replaced_by_the_directory_counts_as_cleared() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("nested"), b"blocker").unwrap();

    clear_dirent_blocking_dir::<ReplacedByWhatBelongs>(root.path(), "nested")
        .expect("the path holds the directory the clearing was for");
}

#[test]
fn a_file_removed_by_the_other_installer_counts_as_cleared() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("nested"), b"blocker").unwrap();

    clear_dirent_blocking_dir::<RemovedByTheOtherInstaller>(root.path(), "nested")
        .expect("nothing stands where the directory belongs");
}

#[test]
fn a_file_still_standing_fails_the_clearing() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("nested"), b"blocker").unwrap();

    clear_dirent_blocking_dir::<LeavesTheBlocker>(root.path(), "nested")
        .expect_err("the blocker is still in the way");
}
