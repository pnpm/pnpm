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
fn needs_build_marker_is_not_used_as_the_completion_marker() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let marker = write_source(&src_root, "needs-build", b"");
    let index = write_source(&src_root, "index.js", b"original");
    let cas = cas_map(&[(crate::NEEDS_BUILD_MARKER, marker), ("index.js", index)]);

    let target = tmp.path().join("pkg");
    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        ImportIndexedDirOpts::default(),
    )
    .expect("fresh import should succeed");

    fs::remove_file(target.join(crate::NEEDS_BUILD_MARKER)).unwrap();
    fs::write(target.join("index.js"), b"built").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        ImportIndexedDirOpts::default(),
    )
    .expect("completed import should stay warm");

    assert_eq!(fs::read(target.join("index.js")).unwrap(), b"built");
    assert!(!target.join(crate::NEEDS_BUILD_MARKER).exists());
}
#[test]
fn safe_to_skip_keeps_a_slot_whose_build_removed_the_needs_build_marker() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"1.0.0\"}");
    let index = write_source(&src_root, "index.js", b"module.exports = 1");
    let needs_build = write_source(&src_root, "needs-build", b"");
    let cas = cas_map(&[
        ("package.json", pkg_json),
        ("index.js", index),
        (crate::NEEDS_BUILD_MARKER, needs_build),
    ]);

    let target = tmp.path().join("slot");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("package.json"), b"{\"version\":\"1.0.0\"}").unwrap();
    fs::write(target.join("index.js"), b"module.exports = 1").unwrap();
    fs::write(target.join("built.node"), b"native addon").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        FORCE_SHARED,
    )
    .expect("a built slot must be left alone");

    assert!(
        target.join("built.node").exists(),
        "the build output must survive; the slot was rebuilt from the staging copy",
    );
    assert!(
        !target.join(crate::NEEDS_BUILD_MARKER).exists(),
        "the consumed needs-build marker must not be put back",
    );
}
