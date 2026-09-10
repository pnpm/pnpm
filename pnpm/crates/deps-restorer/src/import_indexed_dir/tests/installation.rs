use super::{
    super::{ImportIndexedDirOpts, import_indexed_dir},
    FORCE_KEEP, FORCE_SHARED, cas_map, write_source,
};
use pnpm_config::PackageImportMethod;
use pnpm_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use std::{fs, sync::atomic::AtomicU8};
use tempfile::tempdir;

/// Default opts (isolated linker) short-circuit when the target holds the
/// completion marker — the load-bearing invariant that a fully-imported
/// virtual-store slot is never re-imported. A marker-less directory is
/// repaired instead (see [`partial_dir_without_marker_is_repaired`]).
#[test]
fn existing_target_short_circuits_under_default_opts() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let new_pkg_json = write_source(&src_root, "new.json", b"new");
    let cas = cas_map(&[("package.json", new_pkg_json)]);

    let target = tmp.path().join("pkg");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("package.json"), b"old").unwrap();
    fs::write(target.join("extra.txt"), b"keep me").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        ImportIndexedDirOpts::default(),
    )
    .expect("default opts on existing target should be a no-op");

    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"old");
    assert_eq!(fs::read(target.join("extra.txt")).unwrap(), b"keep me");
}
/// The hoisted-linker call site shouldn't hit this in practice, but
/// bailing out would wedge the install.
#[test]
fn force_replaces_regular_file_target() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let file_a = write_source(&src_root, "a.txt", b"contents");
    let cas = cas_map(&[("package.json", file_a)]);

    let target = tmp.path().join("pkg");
    fs::write(&target, b"a file, not a dir").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        FORCE_KEEP,
    )
    .expect("regular-file target should be replaced");

    assert!(target.is_dir(), "target should now be a directory");
    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"contents");
}
/// Sanity-checks that the parent-dir pre-pass is reached on the
/// fresh-target branch (shared between default and force opts).
#[test]
fn fresh_target_creates_nested_directories() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let file_a = write_source(&src_root, "a.txt", b"deep");
    let file_b = write_source(&src_root, "b.txt", b"deeper");
    let cas = cas_map(&[("lib/deep/file.js", file_a), ("lib/deep/nested/file.js", file_b)]);

    let target = tmp.path().join("pkg");
    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        ImportIndexedDirOpts::default(),
    )
    .expect("nested fresh import should succeed");

    assert_eq!(fs::read(target.join("lib/deep/file.js")).unwrap(), b"deep");
    assert_eq!(fs::read(target.join("lib/deep/nested/file.js")).unwrap(), b"deeper");
}
/// Two staging paths produced back-to-back in the same process must
/// differ — otherwise concurrent rayon workers would collide on the
/// rename target. Uses the function indirectly via two force re-installs
/// in parallel.
#[test]
fn concurrent_force_imports_into_different_targets_do_not_collide() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let file_a = write_source(&src_root, "a.txt", b"one");
    let file_b = write_source(&src_root, "b.txt", b"two");
    let cas_a = cas_map(&[("package.json", file_a)]);
    let cas_b = cas_map(&[("package.json", file_b)]);

    let target_a = tmp.path().join("pkg-a");
    let target_b = tmp.path().join("pkg-b");
    // Pre-seed both so the stage-and-swap path is exercised on both.
    fs::create_dir_all(&target_a).unwrap();
    fs::create_dir_all(&target_b).unwrap();
    fs::write(target_a.join("stale.txt"), b"stale").unwrap();
    fs::write(target_b.join("stale.txt"), b"stale").unwrap();

    std::thread::scope(|scope| {
        scope.spawn(|| {
            import_indexed_dir::<SilentReporter>(
                &AtomicU8::new(0),
                PackageImportMethod::Copy,
                &target_a,
                &cas_a,
                FORCE_KEEP,
            )
            .expect("a should succeed");
        });
        scope.spawn(|| {
            import_indexed_dir::<SilentReporter>(
                &AtomicU8::new(0),
                PackageImportMethod::Copy,
                &target_b,
                &cas_b,
                FORCE_KEEP,
            )
            .expect("b should succeed");
        });
    });

    assert_eq!(fs::read(target_a.join("package.json")).unwrap(), b"one");
    assert_eq!(fs::read(target_b.join("package.json")).unwrap(), b"two");
    assert!(!target_a.join("stale.txt").exists());
    assert!(!target_b.join("stale.txt").exists());
}
#[test]
fn partial_dir_without_marker_is_repaired() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"v1");
    let code = write_source(&src_root, "index.js", b"codev1");
    let cas = cas_map(&[("package.json", pkg_json), ("lib/index.js", code)]);

    // Interrupted import: one non-marker file written, marker not yet
    // placed. `leftover.txt` stands in for a concurrent importer's work.
    let target = tmp.path().join("pkg");
    fs::create_dir_all(target.join("lib")).unwrap();
    fs::write(target.join("lib/index.js"), b"codev1").unwrap();
    fs::write(target.join("leftover.txt"), b"keep me").unwrap();
    assert!(!target.join("package.json").exists(), "precondition: marker absent");

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        ImportIndexedDirOpts::default(),
    )
    .expect("a marker-less partial directory must be repaired, not skipped");

    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"v1");
    assert_eq!(fs::read(target.join("lib/index.js")).unwrap(), b"codev1");
    assert_eq!(fs::read(target.join("leftover.txt")).unwrap(), b"keep me");
}
/// pnpm's `pkgExistsAtTargetDir` checks only the marker.
#[test]
fn existing_marker_short_circuits_even_when_other_files_missing() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"vNEW");
    let code = write_source(&src_root, "index.js", b"code");
    let cas = cas_map(&[("package.json", pkg_json), ("lib/index.js", code)]);

    let target = tmp.path().join("pkg");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("package.json"), b"vOLD").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        ImportIndexedDirOpts::default(),
    )
    .expect("marker present should short-circuit");

    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"vOLD", "marker untouched");
    assert!(!target.join("lib/index.js").exists(), "skipped import must not link other files");
}
#[test]
fn fallback_marker_repairs_when_no_package_json() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let a_txt = write_source(&src_root, "a.txt", b"A");
    let b_txt = write_source(&src_root, "b.txt", b"B");
    // No package.json: marker is "a.txt" (lexicographically smallest).
    let cas = cas_map(&[("a.txt", a_txt), ("b.txt", b_txt)]);

    // Partial: the non-marker file is present, the marker ("a.txt") is not.
    let target = tmp.path().join("pkg");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("b.txt"), b"B").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        ImportIndexedDirOpts::default(),
    )
    .expect("missing fallback marker must trigger repair");

    assert_eq!(fs::read(target.join("a.txt")).unwrap(), b"A");
    assert_eq!(fs::read(target.join("b.txt")).unwrap(), b"B");
}
/// A leaked `*_pacquet-stage_*` file would mean the marker rename never
/// happened.
#[test]
fn fresh_import_places_marker_and_leaks_no_temp() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{}");
    let code = write_source(&src_root, "index.js", b"code");
    let cas = cas_map(&[("package.json", pkg_json), ("index.js", code)]);

    let target = tmp.path().join("pkg");
    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        ImportIndexedDirOpts::default(),
    )
    .expect("fresh import should succeed");

    assert!(target.join("package.json").exists(), "marker must be placed");
    for entry in walkdir::WalkDir::new(&target) {
        let path = entry.unwrap().into_path();
        assert!(
            !path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.contains("pacquet-stage")),
            "marker staging temp leaked at {path:?}",
        );
    }
}
/// With the non-marker loop empty, the target must still be created
/// before the marker is staged into it.
#[test]
fn marker_only_map_creates_target_and_places_marker() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{}");
    let cas = cas_map(&[("package.json", pkg_json)]);

    let target = tmp.path().join("pkg");
    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        ImportIndexedDirOpts::default(),
    )
    .expect("marker-only import into a non-existent target should succeed");

    assert!(target.is_dir(), "target directory must be created");
    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"{}", "marker must be placed");
    for entry in walkdir::WalkDir::new(&target) {
        let path = entry.unwrap().into_path();
        assert!(
            !path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.contains("pacquet-stage")),
            "marker staging temp leaked at {path:?}",
        );
    }
}
#[test]
fn safe_to_skip_still_repairs_an_incomplete_target() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"1.0.0\"}");
    let index = write_source(&src_root, "index.js", b"module.exports = 1");
    let cas = cas_map(&[("package.json", pkg_json), ("index.js", index)]);

    let target = tmp.path().join("slot");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("package.json"), b"truncated").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        FORCE_SHARED,
    )
    .expect("an incomplete slot must be repaired");

    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"{\"version\":\"1.0.0\"}");
    assert_eq!(fs::read(target.join("index.js")).unwrap(), b"module.exports = 1");
}
// An incomplete slot is as often an importer mid-flight as an interrupted one, and swapping a
// fresh directory over it would remove the files that importer is still writing.
#[cfg(unix)]
#[test]
fn safe_to_skip_repairs_an_incomplete_target_without_replacing_it() {
    use std::os::unix::fs::MetadataExt;

    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"1.0.0\"}");
    let index = write_source(&src_root, "index.js", b"module.exports = 1");
    let cas = cas_map(&[("package.json", pkg_json), ("index.js", index)]);

    let target = tmp.path().join("slot");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("index.js"), b"module.exports = 1").unwrap();
    let occupied = fs::metadata(&target).unwrap().ino();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        FORCE_SHARED,
    )
    .expect("an incomplete slot must be repaired");

    assert_eq!(fs::metadata(&target).unwrap().ino(), occupied);
    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"{\"version\":\"1.0.0\"}");
    assert_eq!(fs::read(target.join("index.js")).unwrap(), b"module.exports = 1");
}
// Same size, same leading bytes, damaged near the end: the compare has to run past its first
// buffer to see it. The copy tier is the one that reads, since it shares no inode with the store.
#[test]
fn safe_to_skip_repairs_a_file_that_only_differs_past_the_first_read() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let mut whole = vec![b'a'; 40 * 1024];
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"1.0.0\"}");
    let bundle = write_source(&src_root, "bundle.js", &whole);
    let cas = cas_map(&[("package.json", pkg_json), ("bundle.js", bundle)]);

    let target = tmp.path().join("slot");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("package.json"), b"{\"version\":\"1.0.0\"}").unwrap();
    let last = whole.len() - 1;
    whole[last] = b'z';
    fs::write(target.join("bundle.js"), &whole).unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        FORCE_SHARED,
    )
    .expect("a damaged slot must be repaired");

    whole[last] = b'a';
    assert_eq!(fs::read(target.join("bundle.js")).unwrap(), whole);
}
#[test]
fn safe_to_skip_clears_a_file_where_the_package_needs_a_directory() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"1.0.0\"}");
    let index = write_source(&src_root, "index.js", b"module.exports = 1");
    let cas = cas_map(&[("package.json", pkg_json), ("lib/nested/index.js", index)]);

    let target = tmp.path().join("slot");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("lib"), b"a file where a directory belongs").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Hardlink,
        &target,
        &cas,
        FORCE_SHARED,
    )
    .expect("a dirent of the wrong kind must not wedge the install");

    assert_eq!(fs::read(target.join("lib/nested/index.js")).unwrap(), b"module.exports = 1");
}
#[test]
fn safe_to_skip_clears_a_directory_where_the_package_needs_a_file() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"1.0.0\"}");
    let index = write_source(&src_root, "index.js", b"module.exports = 1");
    let cas = cas_map(&[("package.json", pkg_json), ("index.js", index)]);

    let target = tmp.path().join("slot");
    fs::create_dir_all(target.join("index.js")).unwrap();
    fs::write(target.join("index.js").join("stray"), b"leftover").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Hardlink,
        &target,
        &cas,
        FORCE_SHARED,
    )
    .expect("a dirent of the wrong kind must not wedge the install");

    assert_eq!(fs::read(target.join("index.js")).unwrap(), b"module.exports = 1");
}
