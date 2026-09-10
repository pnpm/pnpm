use crate::{FsRename, Host, rename_even_across_devices::rename_even_across_devices};
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

struct PermissionDenied;

impl FsRename for PermissionDenied {
    fn rename(_src: &Path, _dst: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
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

/// The source is the caller's only copy until the removal runs, so a
/// copy that dies partway has to keep it.
#[test]
fn a_failed_copy_keeps_the_source() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("file.txt");
    fs::write(&src, b"contents").unwrap();

    let dst = tmp.path().join("absent-parent/file.txt");
    rename_even_across_devices::<CrossDevice>(&src, &dst).unwrap_err();

    assert_eq!(fs::read(&src).unwrap(), b"contents");
    assert!(!dst.exists());
}

/// The caller that preserves a nested `node_modules/` merges the two
/// directories itself when the imported package ships bundled
/// dependencies. It only gets to do that if the fallback reports the
/// collision instead of copying the old tree over the new one.
#[test]
fn a_cross_device_rename_onto_an_occupied_directory_reports_the_collision() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("node_modules");
    fs::create_dir(&src).unwrap();
    fs::write(src.join("dep.js"), b"// preserved dep").unwrap();

    let dst = tmp.path().join("staged_node_modules");
    fs::create_dir(&dst).unwrap();
    fs::write(dst.join("bundled.js"), b"// bundled dep").unwrap();

    rename_even_across_devices::<CrossDevice>(&src, &dst).unwrap_err();

    assert!(!dst.join("dep.js").exists(), "the destination must be left for the caller to merge");
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
