use crate::{
    FsRemoveDirent, FsRename, Host, rename_even_across_devices::rename_even_across_devices,
};
use std::{fs, io, path::Path};
use tempfile::tempdir;

#[cfg(unix)]
use std::os::unix::fs::symlink;

/// The raw OS code the kernel reports when a rename would cross
/// filesystems. Kept next to the fake so the fake drives exactly the
/// branch [`crate::is_cross_device`] recognises.
fn cross_device_error() -> io::Error {
    #[cfg(unix)]
    return io::Error::from_raw_os_error(18);
    #[cfg(windows)]
    return io::Error::from_raw_os_error(17);
}

/// A filesystem that refuses every rename the way overlayfs refuses to
/// move a directory off a lower layer. No single-volume test fixture
/// can produce that error, so the fallback needs the seam.
struct CrossDevice;

impl FsRename for CrossDevice {
    fn rename(_src: &Path, _dst: &Path) -> io::Result<()> {
        Err(cross_device_error())
    }
}

impl FsRemoveDirent for CrossDevice {
    fn remove_dirent(path: &Path) -> io::Result<()> {
        crate::remove_dirent(path)
    }
}

/// A filesystem that refuses the rename as cross-device and then
/// refuses to remove the source it has just copied.
struct CrossDeviceThenUnremovable;

impl FsRename for CrossDeviceThenUnremovable {
    fn rename(_src: &Path, _dst: &Path) -> io::Result<()> {
        Err(cross_device_error())
    }
}

impl FsRemoveDirent for CrossDeviceThenUnremovable {
    fn remove_dirent(_path: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

struct PermissionDenied;

impl FsRename for PermissionDenied {
    fn rename(_src: &Path, _dst: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    }
}

impl FsRemoveDirent for PermissionDenied {
    fn remove_dirent(_path: &Path) -> io::Result<()> {
        unreachable!("a rename that was refused never reaches the removal")
    }
}

#[test]
fn a_rename_the_filesystem_accepts_is_the_whole_operation() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("index.js"), b"// dep").unwrap();

    let dst = tmp.path().join("dst");
    rename_even_across_devices::<Host>(&src, &dst).unwrap();

    assert!(!src.exists());
    assert_eq!(fs::read(dst.join("index.js")).unwrap(), b"// dep");
}

#[test]
fn a_cross_device_rename_falls_back_to_copying_the_tree_over() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("node_modules");
    fs::create_dir_all(src.join("inner")).unwrap();
    fs::write(src.join("inner/index.js"), b"// inner dep").unwrap();

    let dst = tmp.path().join("staged_node_modules");
    rename_even_across_devices::<CrossDevice>(&src, &dst).unwrap();

    assert_eq!(fs::read(dst.join("inner/index.js")).unwrap(), b"// inner dep");
    assert!(!src.exists(), "the fallback must leave the source moved, not duplicated");
}

#[cfg(unix)]
#[test]
fn the_fallback_moves_symlinks_as_symlinks() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("node_modules");
    fs::create_dir(&src).unwrap();
    symlink("../.pnpm/dep@1.0.0/node_modules/dep", src.join("dep")).unwrap();

    let dst = tmp.path().join("staged_node_modules");
    rename_even_across_devices::<CrossDevice>(&src, &dst).unwrap();

    assert_eq!(
        fs::read_link(dst.join("dep")).unwrap(),
        Path::new("../.pnpm/dep@1.0.0/node_modules/dep"),
    );
}

#[test]
fn a_rename_failure_that_is_not_cross_device_surfaces_untouched() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir(&src).unwrap();

    let dst = tmp.path().join("dst");
    let error = rename_even_across_devices::<PermissionDenied>(&src, &dst).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(src.is_dir(), "a refused rename must leave the source alone");
    assert!(!dst.exists(), "a refused rename must not start a copy");
}

/// A destination whose parent does not exist has nowhere to stage the
/// copy, so the fallback stops before it reads anything.
#[test]
fn a_destination_with_no_parent_directory_keeps_the_source() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("file.txt");
    fs::write(&src, b"contents").unwrap();

    let dst = tmp.path().join("absent-parent/file.txt");
    rename_even_across_devices::<CrossDevice>(&src, &dst).unwrap_err();

    assert_eq!(fs::read(&src).unwrap(), b"contents");
    assert!(!dst.exists());
}

/// The source is the caller's only copy until the destination is
/// committed, so a copy that dies partway has to leave it whole and put
/// nothing at the destination.
#[cfg(unix)]
#[test]
fn a_copy_that_fails_partway_keeps_the_source_and_leaves_no_destination() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let src = tmp.path().join("node_modules");
    let unreadable = src.join("unreadable");
    fs::create_dir_all(&unreadable).unwrap();
    fs::write(src.join("dep.js"), b"// preserved dep").unwrap();
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();

    let dst = tmp.path().join("staged_node_modules");
    let moved = rename_even_across_devices::<CrossDevice>(&src, &dst);

    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o700)).unwrap();

    assert_eq!(moved.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(fs::read(src.join("dep.js")).unwrap(), b"// preserved dep");
    assert!(!dst.exists());
}

/// The caller that preserves a nested `node_modules/` merges the two
/// directories itself when the imported package ships bundled
/// dependencies. It only gets to do that if the fallback reports the
/// collision instead of copying the old tree over the new one. The
/// collision is settled before anything is copied, because the caller
/// answers it by moving the same tree to its backup path — copying
/// first would copy it twice.
#[test]
fn a_cross_device_rename_onto_an_occupied_directory_reports_the_collision() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("node_modules");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("dep.js"), b"// preserved dep").unwrap();

    let dst = tmp.path().join("staged_node_modules");
    fs::create_dir(&dst).unwrap();
    fs::write(dst.join("bundled.js"), b"// bundled dep").unwrap();

    let error = rename_even_across_devices::<CrossDevice>(&src, &dst).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::DirectoryNotEmpty);
    assert!(!dst.join("dep.js").exists(), "the destination must be left for the caller to merge");
    assert_eq!(fs::read(src.join("dep.js")).unwrap(), b"// preserved dep");
}

/// The collision has to be settled before the copy, not after it: an
/// unreadable source that still reports the occupied destination proves
/// nothing was read.
#[cfg(unix)]
#[test]
fn an_occupied_destination_is_reported_without_reading_the_source() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let src = tmp.path().join("node_modules");
    fs::create_dir(&src).unwrap();
    fs::set_permissions(&src, fs::Permissions::from_mode(0o000)).unwrap();

    let dst = tmp.path().join("staged_node_modules");
    fs::create_dir(&dst).unwrap();
    fs::write(dst.join("bundled.js"), b"// bundled dep").unwrap();

    let error = rename_even_across_devices::<CrossDevice>(&src, &dst).unwrap_err();

    fs::set_permissions(&src, fs::Permissions::from_mode(0o700)).unwrap();

    assert_eq!(error.kind(), io::ErrorKind::DirectoryNotEmpty);
}

/// An empty destination directory is one a POSIX rename would take, so
/// the fallback has to take it too. Windows is the other way round:
/// `MoveFileExW` refuses an existing directory whether or not it is
/// empty, and the fallback's rename refuses it for the same reason, so
/// there is no behavior to pin there.
#[cfg(unix)]
#[test]
fn a_cross_device_rename_onto_an_empty_directory_still_moves() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("node_modules");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("dep.js"), b"// preserved dep").unwrap();

    let dst = tmp.path().join("staged_node_modules");
    fs::create_dir(&dst).unwrap();

    rename_even_across_devices::<CrossDevice>(&src, &dst).unwrap();

    assert_eq!(fs::read(dst.join("dep.js")).unwrap(), b"// preserved dep");
    assert!(!src.exists());
}

/// Once the copy is at the destination the move has happened.
/// Reporting the removal failure instead would send the caller looking
/// for the data at the source, where a recursive removal may already
/// have deleted part of it, and its cleanup would drop the complete
/// copy.
#[test]
fn a_source_that_cannot_be_removed_does_not_undo_the_move() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("node_modules");
    fs::create_dir_all(src.join("inner")).unwrap();
    fs::write(src.join("inner/index.js"), b"// inner dep").unwrap();

    let dst = tmp.path().join("staged_node_modules");
    rename_even_across_devices::<CrossDeviceThenUnremovable>(&src, &dst)
        .expect("the destination is committed, so the move succeeded");

    assert_eq!(fs::read(dst.join("inner/index.js")).unwrap(), b"// inner dep");
}

/// A destination that cannot be read is the caller's problem, not a
/// reason to copy a whole tree and let the rename run into it.
#[cfg(unix)]
#[test]
fn an_unreadable_destination_surfaces_instead_of_being_copied_into() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let src = tmp.path().join("node_modules");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("dep.js"), b"// preserved dep").unwrap();

    let dst = tmp.path().join("staged_node_modules");
    fs::create_dir(&dst).unwrap();
    fs::set_permissions(&dst, fs::Permissions::from_mode(0o000)).unwrap();

    let moved = rename_even_across_devices::<CrossDevice>(&src, &dst);

    fs::set_permissions(&dst, fs::Permissions::from_mode(0o700)).unwrap();

    assert_eq!(moved.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(fs::read(src.join("dep.js")).unwrap(), b"// preserved dep");
}

/// The staging copy is a dirent of its own next to the destination.
/// Leaving one behind would put a stray directory inside a
/// `node_modules/`, where the next install would have to reason about
/// it.
#[test]
fn the_fallback_leaves_no_staging_dirent_behind() {
    let tmp = tempdir().unwrap();
    let parent = tmp.path().join("parent");
    fs::create_dir(&parent).unwrap();
    let src = parent.join("src");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("index.js"), b"// dep").unwrap();

    let occupied = parent.join("occupied");
    fs::create_dir(&occupied).unwrap();
    fs::write(occupied.join("other.js"), b"// other").unwrap();
    rename_even_across_devices::<CrossDevice>(&src, &occupied).unwrap_err();

    let free = parent.join("free");
    rename_even_across_devices::<CrossDevice>(&src, &free).unwrap();

    let mut remaining: Vec<_> = fs::read_dir(&parent)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    remaining.sort();
    assert_eq!(remaining, ["free", "occupied"]);
}
