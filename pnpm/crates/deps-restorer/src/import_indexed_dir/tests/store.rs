#[cfg(unix)]
use super::FORCE_SHARED_KEEP;
use super::{
    super::{ImportIndexedDirOpts, import_indexed_dir},
    FORCE_SHARED, SHARED, cas_map, write_source,
};
use pnpm_config::PackageImportMethod;
use pnpm_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use std::{fs, sync::atomic::AtomicU8};
use tempfile::tempdir;

#[test]
fn safe_to_skip_does_not_accept_a_slot_that_is_still_being_written() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"1.0.0\"}");
    let index = write_source(&src_root, "index.js", b"module.exports = 1");
    let cas = cas_map(&[("package.json", pkg_json), ("index.js", index)]);

    let target = tmp.path().join("slot");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("index.js"), b"half-written").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        FORCE_SHARED,
    )
    .expect("an unfinished slot must be completed, not accepted");

    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"{\"version\":\"1.0.0\"}");
    assert_eq!(
        fs::read(target.join("index.js")).unwrap(),
        b"module.exports = 1",
        "the half-written file must be replaced, not kept",
    );
}
// The marker says nothing about the files placed before it: a slot damaged after its import
// finished carries a complete-looking tree, and existence checks would call it done forever.
#[test]
fn safe_to_skip_repairs_a_slot_damaged_after_it_was_completed() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"1.0.0\"}");
    let index = write_source(&src_root, "index.js", b"module.exports = 1");
    let cas = cas_map(&[("package.json", pkg_json), ("index.js", index)]);

    let target = tmp.path().join("slot");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("package.json"), b"{\"version\":\"1.0.0\"}").unwrap();
    fs::write(target.join("index.js"), b"corrupted after the import").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Hardlink,
        &target,
        &cas,
        FORCE_SHARED,
    )
    .expect("a damaged slot must be repaired");

    assert_eq!(fs::read(target.join("index.js")).unwrap(), b"module.exports = 1");
}
// A package with bundled dependencies ships its own node_modules/, and the interrupted-build
// call shape asks for it to be preserved. Repairing in place preserves it by never removing
// anything, so the slot must not take the staging swap that a shared slot is not allowed to run.
#[cfg(unix)]
#[test]
fn safe_to_skip_repairs_a_slot_holding_a_nested_node_modules_in_place() {
    use std::os::unix::fs::MetadataExt;

    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"1.0.0\"}");
    let index = write_source(&src_root, "index.js", b"module.exports = 1");
    let cas = cas_map(&[("package.json", pkg_json), ("index.js", index)]);

    let target = tmp.path().join("slot");
    let bundled = target.join("node_modules").join("bundled");
    fs::create_dir_all(&bundled).unwrap();
    fs::write(bundled.join("index.js"), b"bundled dependency").unwrap();
    let occupied = fs::metadata(&target).unwrap().ino();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Hardlink,
        &target,
        &cas,
        FORCE_SHARED_KEEP,
    )
    .expect("an incomplete slot must be repaired");

    assert_eq!(fs::metadata(&target).unwrap().ino(), occupied);
    assert_eq!(fs::read(bundled.join("index.js")).unwrap(), b"bundled dependency");
    assert_eq!(fs::read(target.join("index.js")).unwrap(), b"module.exports = 1");
    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"{\"version\":\"1.0.0\"}");
}
#[test]
fn safe_to_skip_imports_into_an_absent_shared_slot() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"1.0.0\"}");
    let index = write_source(&src_root, "index.js", b"module.exports = 1");
    let cas = cas_map(&[("package.json", pkg_json), ("lib/index.js", index)]);

    let target = tmp.path().join("nested").join("slot");
    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Hardlink,
        &target,
        &cas,
        SHARED,
    )
    .expect("an absent shared slot must be created and filled");

    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"{\"version\":\"1.0.0\"}");
    assert_eq!(fs::read(target.join("lib/index.js")).unwrap(), b"module.exports = 1");
}
// The isolated linker imports without `force`, so the marker-less repair is where a shared slot
// left half-written by an importer that died is met on the next install.
#[test]
fn shared_slot_repair_without_force_replaces_a_damaged_file() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"1.0.0\"}");
    let index = write_source(&src_root, "index.js", b"module.exports = 1");
    let cas = cas_map(&[("package.json", pkg_json), ("index.js", index)]);

    let target = tmp.path().join("slot");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("index.js"), b"half-written").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Hardlink,
        &target,
        &cas,
        SHARED,
    )
    .expect("a marker-less shared slot must be repaired");

    assert_eq!(fs::read(target.join("index.js")).unwrap(), b"module.exports = 1");
    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"{\"version\":\"1.0.0\"}");
}
// A private slot holds only this install's own interrupted work, so its repair keeps adopting.
#[test]
fn private_slot_repair_without_force_adopts_what_is_there() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"1.0.0\"}");
    let index = write_source(&src_root, "index.js", b"module.exports = 1");
    let cas = cas_map(&[("package.json", pkg_json), ("index.js", index)]);

    let target = tmp.path().join("slot");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("index.js"), b"half-written").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Hardlink,
        &target,
        &cas,
        ImportIndexedDirOpts::default(),
    )
    .expect("a marker-less private slot must be repaired");

    assert_eq!(fs::read(target.join("index.js")).unwrap(), b"half-written");
    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"{\"version\":\"1.0.0\"}");
}
