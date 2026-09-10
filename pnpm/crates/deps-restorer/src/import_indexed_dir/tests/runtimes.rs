#[cfg(unix)]
use super::super::ImportIndexedDirError;
use super::{super::import_indexed_dir, FORCE_KEEP, FORCE_ONLY, cas_map, write_source};
use pnpm_config::PackageImportMethod;
use pnpm_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use std::{fs, sync::atomic::AtomicU8};
use tempfile::tempdir;

#[test]
fn force_keep_replaces_files_and_preserves_node_modules() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"2.0.0\"}");
    let cas = cas_map(&[("package.json", pkg_json)]);

    let target = tmp.path().join("pkg");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("package.json"), b"{\"version\":\"1.0.0\"}").unwrap();
    fs::write(target.join("stale.txt"), b"left over from v1").unwrap();
    fs::create_dir_all(target.join("node_modules/inner")).unwrap();
    fs::write(target.join("node_modules/inner/index.js"), b"// inner dep").unwrap();
    fs::write(target.join("node_modules/.placeholder"), b"keep me").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        FORCE_KEEP,
    )
    .expect("overwrite should succeed");

    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"{\"version\":\"2.0.0\"}");
    assert!(!target.join("stale.txt").exists(), "stale file must be removed");
    assert_eq!(fs::read(target.join("node_modules/inner/index.js")).unwrap(), b"// inner dep");
    assert_eq!(fs::read(target.join("node_modules/.placeholder")).unwrap(), b"keep me");
}
/// This isn't a call shape any current pacquet linker uses, but the
/// parameter space requires it: `force=true, keep_modules_dir=false` is
/// a valid `ImportIndexedDirOpts` and matches pnpm's `importIndexedDir(...,
/// { force: true })` without the `keepModulesDir` flag.
#[test]
fn force_without_keep_clobbers_node_modules() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"v2");
    let cas = cas_map(&[("package.json", pkg_json)]);

    let target = tmp.path().join("pkg");
    fs::create_dir_all(target.join("node_modules/inner")).unwrap();
    fs::write(target.join("node_modules/inner/dep.js"), b"// old dep").unwrap();
    fs::write(target.join("package.json"), b"v1").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        FORCE_ONLY,
    )
    .expect("force overwrite should succeed");

    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"v2");
    assert!(
        !target.join("node_modules").exists(),
        "without keep_modules_dir, node_modules/ must be removed too",
    );
}
/// Exercises the "preserve" branch when there's nothing to preserve.
#[test]
fn force_keep_without_node_modules_replaces_cleanly() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"new");
    let cas = cas_map(&[("package.json", pkg_json)]);

    let target = tmp.path().join("pkg");
    fs::create_dir_all(target.join("nested")).unwrap();
    fs::write(target.join("nested/old.txt"), b"old").unwrap();
    fs::write(target.join("top.txt"), b"top").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        FORCE_KEEP,
    )
    .expect("overwrite should succeed");

    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"new");
    assert!(!target.join("nested").exists(), "stale nested dir must be removed");
    assert!(!target.join("top.txt").exists(), "stale top-level file must be removed");
}
#[test]
fn node_modules_collision_in_file_map_merges() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let regular = write_source(&src_root, "a.txt", b"top");
    let inside_nm = write_source(&src_root, "b.txt", b"shipped-nm");
    let cas = cas_map(&[("package.json", regular), ("node_modules/foo/index.js", inside_nm)]);

    let target = tmp.path().join("pkg");
    fs::create_dir_all(target.join("node_modules/existing")).unwrap();
    fs::write(target.join("node_modules/existing/keep.js"), b"survivor").unwrap();
    fs::create_dir_all(target.join("node_modules/foo")).unwrap();
    fs::write(target.join("node_modules/foo/stale.js"), b"replaced").unwrap();

    import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        FORCE_KEEP,
    )
    .expect("bundled and preserved node_modules should merge");

    assert_eq!(fs::read(target.join("node_modules/existing/keep.js")).unwrap(), b"survivor");
    assert_eq!(fs::read(target.join("node_modules/foo/index.js")).unwrap(), b"shipped-nm");
    assert!(!target.join("node_modules/foo/stale.js").exists());
}
/// Data-loss regression: if `remove_dir_all(dir_path)` fails *after*
/// the preserved `node_modules/` has been moved into the staging
/// directory, the staged copy must be restored to its original path
/// before the staging directory is cleaned up. Otherwise the
/// best-effort cleanup would silently destroy the user's nested deps.
///
/// We force the removal to fail by chmod'ing a subdirectory inside
/// `dir_path` to 0o500: `remove_dir_all` recurses into it and can read
/// its entries, but unlinking those entries needs write on the
/// containing dir, which 0o500 denies. That fails after `node_modules`
/// has been moved into staging (step 3) but before the swap (step 5),
/// so the rescue path is exactly what the assertions exercise.
#[test]
#[cfg(unix)]
fn remove_dir_all_failure_restores_preserved_node_modules() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"new");
    let bundled = write_source(&src_root, "bundled.js", b"bundled");
    let cas = cas_map(&[("package.json", pkg_json), ("node_modules/inner/bundled.js", bundled)]);

    let target = tmp.path().join("pkg");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("stale.txt"), b"stale").unwrap();
    fs::create_dir_all(target.join("node_modules/inner")).unwrap();
    fs::write(target.join("node_modules/inner/sentinel"), b"survivor").unwrap();

    // Create a write-protected subdirectory whose contents
    // `remove_dir_all` can read but not unlink.
    let locked = target.join("locked_subdir");
    fs::create_dir_all(&locked).unwrap();
    fs::write(locked.join("immutable.txt"), b"locked").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();

    let err = import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        FORCE_KEEP,
    )
    .expect_err("RemoveExisting should fire");

    // Restore perms so the tempdir teardown can succeed regardless of
    // what state the failed swap left the tree in.
    if locked.exists() {
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
    }

    assert!(matches!(err, ImportIndexedDirError::RemoveExisting { .. }), "got: {err:?}");
    // The rescue path must have moved `stage/node_modules/` back onto
    // `target/node_modules/` before the cleanup rimrafed staging.
    assert!(
        target.join("node_modules/inner/sentinel").exists(),
        "preserved node_modules/ must survive the failed swap",
    );
    assert_eq!(
        fs::read(target.join("node_modules/inner/sentinel")).unwrap(),
        b"survivor",
        "preserved node_modules/ contents must be intact",
    );
    assert!(
        !target.join("node_modules/inner/bundled.js").exists(),
        "a failed swap must restore the conflicting dependency tree",
    );
    // No staging directory left behind anywhere under the outer
    // tempdir.
    for entry in walkdir::WalkDir::new(tmp.path()) {
        let path = entry.unwrap().into_path();
        assert!(
            !path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.contains("pacquet-stage")),
            "staging directory leaked at {path:?}",
        );
    }
}
/// `symlink_metadata` errors on the preserved `node_modules/` inspect
/// must surface as `InspectTarget`, not be swallowed as "nothing to
/// preserve". Swallowing them would silently clobber nested deps when
/// the swap removes `dir_path`, masking real filesystem problems
/// (permission errors, transient I/O failures).
///
/// We drive a `PermissionDenied` by stripping search permission from
/// `dir_path` itself — `symlink_metadata` on `dir_path/node_modules`
/// needs search-execute on `dir_path` to resolve the child path. The
/// outer `symlink_metadata(dir_path)` call resolves against the parent
/// (which we leave alone), so dispatch still routes us into
/// `stage_and_swap`.
#[test]
#[cfg(unix)]
fn node_modules_inspect_permission_denied_surfaces() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"new");
    let cas = cas_map(&[("package.json", pkg_json)]);

    let target = tmp.path().join("pkg");
    fs::create_dir_all(&target).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o000)).unwrap();

    let err = import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        FORCE_KEEP,
    )
    .expect_err("InspectTarget should fire");

    // Restore perms so the tempdir teardown can succeed.
    fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();

    assert!(matches!(err, ImportIndexedDirError::InspectTarget { .. }), "got: {err:?}");
    // No staging directory should be left behind — the early-error
    // cleanup must have rimrafed it.
    for entry in walkdir::WalkDir::new(tmp.path()) {
        let path = entry.unwrap().into_path();
        assert!(
            !path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.contains("pacquet-stage")),
            "staging directory leaked at {path:?}",
        );
    }
}
