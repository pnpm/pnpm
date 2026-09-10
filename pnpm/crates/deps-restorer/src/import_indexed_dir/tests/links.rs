#[cfg(unix)]
use super::FORCE_KEEP;
use super::{
    super::{ImportIndexedDirOpts, import_indexed_dir},
    FORCE_SHARED, cas_map, write_source,
};
use pnpm_config::PackageImportMethod;
use pnpm_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use std::{fs, sync::atomic::AtomicU8};
use tempfile::tempdir;

#[test]
fn fresh_target_links_files() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let file_a = write_source(&src_root, "a.txt", b"alpha");
    let file_b = write_source(&src_root, "b.txt", b"beta");
    let cas = cas_map(&[("package.json", file_a), ("lib/index.js", file_b)]);

    let target = tmp.path().join("pkg");
    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        ImportIndexedDirOpts::default(),
    )
    .expect("fresh import should succeed");

    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"alpha");
    assert_eq!(fs::read(target.join("lib/index.js")).unwrap(), b"beta");
}
#[test]
#[cfg(unix)]
fn force_replaces_symlink_target_without_following() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let file_a = write_source(&src_root, "a.txt", b"new");
    let cas = cas_map(&[("package.json", file_a)]);

    // Make a real directory elsewhere with a file we don't want
    // overwritten, then point `target` at it via a symlink.
    let pointee = tmp.path().join("real_dir");
    fs::create_dir_all(&pointee).unwrap();
    fs::write(pointee.join("sentinel.txt"), b"untouched").unwrap();
    let target = tmp.path().join("pkg");
    std::os::unix::fs::symlink(&pointee, &target).unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        FORCE_KEEP,
    )
    .expect("symlink target should be replaced");

    let target_meta = fs::symlink_metadata(&target).unwrap();
    assert!(target_meta.file_type().is_dir(), "target is now a real directory");
    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"new");
    assert_eq!(fs::read(pointee.join("sentinel.txt")).unwrap(), b"untouched");
}
/// On Unix, when `Hardlink` is available we want force re-imports to
/// share inodes with the freshly-staged source so re-installs benefit
/// from the same store-sharing as fresh installs. Doubles as proof
/// that the staging-rename path doesn't silently downgrade to copy.
#[test]
#[cfg(unix)]
fn hardlink_method_survives_staging_swap() {
    use std::os::unix::fs::MetadataExt;

    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let src = write_source(&src_root, "a.txt", b"shared");
    let cas = cas_map(&[("package.json", src.clone())]);

    let target = tmp.path().join("pkg");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("package.json"), b"stale").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Hardlink,
        &target,
        &cas,
        FORCE_KEEP,
    )
    .expect("hardlink import should succeed on same-FS tempdir");

    let src_ino = fs::metadata(&src).unwrap().ino();
    let dst_ino = fs::metadata(target.join("package.json")).unwrap().ino();
    assert_eq!(src_ino, dst_ino, "hardlinked re-import must share inode with the store source");
}
// `fs::copy` overwrites, so only a linking tier can adopt a damaged file and keep it.
#[test]
fn safe_to_skip_replaces_a_damaged_file_the_linking_tiers_would_adopt() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"1.0.0\"}");
    let index = write_source(&src_root, "index.js", b"module.exports = 1");
    let cas = cas_map(&[("package.json", pkg_json), ("index.js", index)]);

    let target = tmp.path().join("slot");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("index.js"), b"half-written").unwrap();
    fs::write(target.join("build.node"), b"output of an interrupted build").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Hardlink,
        &target,
        &cas,
        FORCE_SHARED,
    )
    .expect("an unfinished slot must be completed, not accepted");

    assert_eq!(fs::read(target.join("index.js")).unwrap(), b"module.exports = 1");
    assert!(
        target.join("build.node").exists(),
        "a file the package does not declare is not ours to remove from a shared slot",
    );
}
#[cfg(unix)]
#[test]
fn safe_to_skip_replaces_a_symlink_to_matching_store_content() {
    use std::os::unix::fs::symlink;

    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"1.0.0\"}");
    let index = write_source(&src_root, "index.js", b"module.exports = 1");
    let cas = cas_map(&[("package.json", pkg_json), ("index.js", index.clone())]);

    let target = tmp.path().join("slot");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("package.json"), b"{\"version\":\"1.0.0\"}").unwrap();
    symlink(&index, target.join("index.js")).unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Hardlink,
        &target,
        &cas,
        FORCE_SHARED,
    )
    .expect("a shared slot must not adopt a symlink to store content");

    assert!(!fs::symlink_metadata(target.join("index.js")).unwrap().file_type().is_symlink());
    assert_eq!(fs::read(target.join("index.js")).unwrap(), b"module.exports = 1");
}
