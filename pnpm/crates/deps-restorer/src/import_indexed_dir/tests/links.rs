#[cfg(unix)]
use super::{super::ImportIndexedDirError, FORCE_KEEP};
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

    assert!(
        !fs::symlink_metadata(target.join("index.js"))
            .unwrap()
            .file_type()
            .is_symlink(),
    );
    assert_eq!(fs::read(target.join("index.js")).unwrap(), b"module.exports = 1");
}

#[cfg(unix)]
#[test]
fn preserve_symlinks_preserves_symlinked_package_json() {
    use std::os::unix::fs::symlink;

    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("src");
    fs::create_dir_all(&src_root).unwrap();
    let real_pkg_json = write_source(&src_root, "real_package.json", b"{\"name\":\"pkg\"}");
    let link_pkg_json = src_root.join("package.json");
    symlink("real_package.json", &link_pkg_json).unwrap();
    let cas = cas_map(&[("real_package.json", real_pkg_json), ("package.json", link_pkg_json)]);

    let target = tmp.path().join("target");
    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        ImportIndexedDirOpts { preserve_symlinks: true, ..ImportIndexedDirOpts::default() },
    )
    .expect("import with preserve_symlinks should succeed");

    let target_meta = fs::symlink_metadata(target.join("package.json")).unwrap();
    assert!(target_meta.file_type().is_symlink(), "package.json marker must remain a symlink");
    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"{\"name\":\"pkg\"}");
}

#[cfg(unix)]
#[test]
fn preserve_symlinks_relocates_absolute_symlink() {
    use std::os::unix::fs::symlink;

    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("src");
    fs::create_dir_all(&src_root).unwrap();
    let real_file = write_source(&src_root, "real.txt", b"content");
    let link_file = src_root.join("link.txt");
    symlink(&real_file, &link_file).unwrap();
    let cas = cas_map(&[("real.txt", real_file), ("link.txt", link_file)]);

    let target = tmp.path().join("target");
    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        ImportIndexedDirOpts { preserve_symlinks: true, ..ImportIndexedDirOpts::default() },
    )
    .expect("import with preserve_symlinks should succeed");

    let target_meta = fs::symlink_metadata(target.join("link.txt")).unwrap();
    assert!(target_meta.file_type().is_symlink(), "link.txt must remain a symlink");
    let target_link = fs::read_link(target.join("link.txt")).unwrap();
    assert_eq!(target_link, std::path::Path::new("real.txt"));
    assert_eq!(fs::read(target.join("link.txt")).unwrap(), b"content");
}

#[cfg(unix)]
#[test]
fn preserve_symlinks_rejects_escaping_symlink() {
    use std::os::unix::fs::symlink;

    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("src");
    fs::create_dir_all(&src_root).unwrap();
    let real_file = write_source(&src_root, "real.txt", b"content");
    let link_file = src_root.join("link.txt");
    symlink(&real_file, &link_file).unwrap();
    let cas = cas_map(&[("real.txt", real_file), ("link.txt", link_file.clone())]);

    fs::remove_file(&link_file).unwrap();
    symlink("../../outside", &link_file).unwrap();

    let target = tmp.path().join("target");
    let err = import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        ImportIndexedDirOpts { preserve_symlinks: true, ..ImportIndexedDirOpts::default() },
    )
    .expect_err("import with escaping symlink should fail");

    assert!(
        matches!(err, ImportIndexedDirError::SymlinkTargetEscapes { .. }),
        "expected SymlinkTargetEscapes error, got {err:?}",
    );
    assert!(
        fs::symlink_metadata(target.join("link.txt")).is_err(),
        "destination link must not be created",
    );
}

#[cfg(unix)]
#[test]
fn preserve_symlinks_imports_a_link_to_a_left_out_file_as_that_file() {
    use std::os::unix::fs::symlink;

    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("src");
    fs::create_dir_all(&src_root).unwrap();
    write_source(&src_root, "real.txt", b"content");
    let link_file = src_root.join("link.txt");
    symlink("real.txt", &link_file).unwrap();
    let cas = cas_map(&[("link.txt", link_file)]);

    let target = tmp.path().join("target");
    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        ImportIndexedDirOpts { preserve_symlinks: true, ..ImportIndexedDirOpts::default() },
    )
    .expect("import with preserve_symlinks should succeed");

    assert!(fs::symlink_metadata(target.join("link.txt")).unwrap().is_file());
    assert_eq!(fs::read(target.join("link.txt")).unwrap(), b"content");
}

#[cfg(unix)]
#[test]
fn preserve_symlinks_leaves_out_a_directory_link_to_a_left_out_directory() {
    use std::os::unix::fs::symlink;

    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("src");
    write_source(&src_root, "sub/nested.txt", b"nested");
    let link = src_root.join("dir-link");
    symlink("sub", &link).unwrap();
    let cas = cas_map(&[("dir-link", link)]);

    let target = tmp.path().join("target");
    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        ImportIndexedDirOpts { preserve_symlinks: true, ..ImportIndexedDirOpts::default() },
    )
    .expect("import with preserve_symlinks should succeed");

    assert!(fs::symlink_metadata(target.join("dir-link")).is_err());
}

#[cfg(unix)]
#[test]
fn preserve_symlinks_relativizes_an_absolute_link_through_a_linked_root() {
    use std::os::unix::fs::symlink;

    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("src");
    let real_file = write_source(&src_root, "real.txt", b"content");
    let root_link = tmp.path().join("src-link");
    symlink(&src_root, &root_link).unwrap();
    let link_file = src_root.join("link.txt");
    symlink(root_link.join("real.txt"), &link_file).unwrap();
    let cas = cas_map(&[("real.txt", real_file), ("link.txt", link_file)]);

    let target = tmp.path().join("target");
    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        ImportIndexedDirOpts { preserve_symlinks: true, ..ImportIndexedDirOpts::default() },
    )
    .expect("import with preserve_symlinks should succeed");

    assert_eq!(fs::read_link(target.join("link.txt")).unwrap(), std::path::Path::new("real.txt"));
}

#[cfg(windows)]
#[test]
fn preserve_symlinks_recreates_an_internal_junction() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("src");
    let nested = write_source(&src_root, "sub/nested.txt", b"nested");
    let link = src_root.join("dir-link");
    junction::create(src_root.join("sub"), &link).unwrap();
    let cas = cas_map(&[("sub/nested.txt", nested), ("dir-link", link)]);

    let target = tmp.path().join("target");
    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        ImportIndexedDirOpts { preserve_symlinks: true, ..ImportIndexedDirOpts::default() },
    )
    .expect("import with preserve_symlinks should succeed");

    assert!(pnpm_fs::is_symlink_or_junction(&target.join("dir-link")).unwrap());
    assert_eq!(fs::read(target.join("dir-link/nested.txt")).unwrap(), b"nested");
}
