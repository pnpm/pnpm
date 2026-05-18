use super::{ImportIndexedDirError, ImportIndexedDirOpts, import_indexed_dir};
use pacquet_config::PackageImportMethod;
use pacquet_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicU8,
};
use tempfile::tempdir;

fn write_source(dir: &Path, rel: &str, contents: &[u8]) -> PathBuf {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create source parent");
    }
    fs::write(&path, contents).expect("write source file");
    path
}

fn cas_map(entries: &[(&str, PathBuf)]) -> HashMap<String, PathBuf> {
    entries.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
}

/// Force re-imports both with and without `keep_modules_dir` go down
/// the same stage-and-swap path. Bundle them here so the call sites
/// stay terse.
const FORCE_KEEP: ImportIndexedDirOpts =
    ImportIndexedDirOpts { force: true, keep_modules_dir: true };
const FORCE_ONLY: ImportIndexedDirOpts =
    ImportIndexedDirOpts { force: true, keep_modules_dir: false };

/// Smoke test: with no existing target and default opts (matching the
/// isolated linker's call shape), populate the directory like
/// upstream `tryImportIndexedDir`. The opts don't matter here because
/// the fresh-target branch is shared.
#[test]
fn fresh_target_links_files() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let a = write_source(&src_root, "a.txt", b"alpha");
    let b = write_source(&src_root, "b.txt", b"beta");
    let cas = cas_map(&[("package.json", a), ("lib/index.js", b)]);

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

/// Default opts (isolated linker) must short-circuit when the target
/// already exists. This is the load-bearing invariant for the virtual
/// store: each slot is populated exactly once and never re-imported.
/// A regression here would cause the isolated linker to do redundant
/// work and possibly clobber working state.
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

    // Nothing was touched.
    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"old");
    assert_eq!(fs::read(target.join("extra.txt")).unwrap(), b"keep me");
}

/// The defining test for the hoisted-linker call shape: a re-install
/// with `force` + `keep_modules_dir` must replace every file in the
/// package directory but leave a pre-existing `node_modules/`
/// subdirectory (and everything inside it) untouched. Models the
/// hoisted-linker pattern of "rimraf orphans, then re-import each
/// package over the top, where some packages have already had their
/// nested deps installed by a sibling pass".
#[test]
fn force_keep_replaces_files_and_preserves_node_modules() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"2.0.0\"}");
    let cas = cas_map(&[("package.json", pkg_json)]);

    let target = tmp.path().join("pkg");
    // Pre-existing package state: a stale package.json plus a nested
    // node_modules/ that must not be clobbered.
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

    // New file in place, stale file evicted.
    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"{\"version\":\"2.0.0\"}");
    assert!(!target.join("stale.txt").exists(), "stale file must be removed");
    // Nested deps preserved verbatim — both files and the directory
    // structure intact.
    assert_eq!(fs::read(target.join("node_modules/inner/index.js")).unwrap(), b"// inner dep");
    assert_eq!(fs::read(target.join("node_modules/.placeholder")).unwrap(), b"keep me");
}

/// With `force` but not `keep_modules_dir`, an existing
/// `node_modules/` is removed along with everything else. This isn't
/// a call shape any current pacquet linker uses, but the parameter
/// space requires it: `force=true, keep_modules_dir=false` is a valid
/// `ImportIndexedDirOpts` and matches pnpm's `importIndexedDir(...,
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

/// If the package directory exists but has no `node_modules/`, the
/// force re-install still wipes the stale files. Variant of the
/// previous test that exercises the "preserve" branch when there's
/// nothing to preserve.
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

/// A regular file occupying the target path is replaced with the
/// freshly-imported directory under `force`. The hoisted-linker call
/// site shouldn't hit this in practice, but bailing out would wedge
/// the install.
#[test]
fn force_replaces_regular_file_target() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let a = write_source(&src_root, "a.txt", b"contents");
    let cas = cas_map(&[("package.json", a)]);

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

/// A symlink occupying the target path is unlinked (not followed)
/// under `force` and replaced with the imported package. The
/// pointee — if it exists — must not be touched.
#[test]
#[cfg(unix)]
fn force_replaces_symlink_target_without_following() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let a = write_source(&src_root, "a.txt", b"new");
    let cas = cas_map(&[("package.json", a)]);

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
    // The original pointee must still contain its sentinel.
    assert_eq!(fs::read(pointee.join("sentinel.txt")).unwrap(), b"untouched");
}

/// Deeply-nested files in the indexed map land in the right places on
/// a fresh install. Sanity-checks that the parent-dir pre-pass is
/// reached on the fresh-target branch (shared between default and
/// force opts).
#[test]
fn fresh_target_creates_nested_directories() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let a = write_source(&src_root, "a.txt", b"deep");
    let b = write_source(&src_root, "b.txt", b"deeper");
    let cas = cas_map(&[("lib/deep/file.js", a), ("lib/deep/nested/file.js", b)]);

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

/// If the indexed file map names a `node_modules/...` entry while the
/// destination already has a real `node_modules/` to preserve, surface
/// the collision rather than silently merging. Upstream's
/// `moveOrMergeModulesDirs` would merge; pacquet's slice-1 consumer
/// (the hoisted-linker) never produces this state, so erroring loudly
/// is the right call until a real caller demands the merge.
#[test]
fn node_modules_collision_in_file_map_errors() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let regular = write_source(&src_root, "a.txt", b"top");
    let inside_nm = write_source(&src_root, "b.txt", b"shipped-nm");
    let cas = cas_map(&[("package.json", regular), ("node_modules/foo/index.js", inside_nm)]);

    let target = tmp.path().join("pkg");
    fs::create_dir_all(target.join("node_modules/existing")).unwrap();
    fs::write(target.join("node_modules/existing/keep.js"), b"survivor").unwrap();

    let err = import_indexed_dir::<SilentReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        FORCE_KEEP,
    )
    .expect_err("collision should surface");
    assert!(matches!(err, ImportIndexedDirError::NodeModulesCollision { .. }), "got: {err:?}");

    // After the error, the existing nested dep must still be on disk —
    // the function's cleanup must not have rimrafed it as a side
    // effect of the failed stage.
    assert_eq!(fs::read(target.join("node_modules/existing/keep.js")).unwrap(), b"survivor");
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
    let cas = cas_map(&[("package.json", pkg_json)]);

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

/// Two staging paths produced back-to-back in the same process must
/// differ — otherwise concurrent rayon workers would collide on the
/// rename target. Uses the function indirectly via two force re-installs
/// in parallel.
#[test]
fn concurrent_force_imports_into_different_targets_do_not_collide() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let a = write_source(&src_root, "a.txt", b"one");
    let b = write_source(&src_root, "b.txt", b"two");
    let cas_a = cas_map(&[("package.json", a)]);
    let cas_b = cas_map(&[("package.json", b)]);

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
