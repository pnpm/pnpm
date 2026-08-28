use super::{ImportIndexedDirError, ImportIndexedDirOpts, claim_dir, import_indexed_dir};
use pnpm_config::PackageImportMethod;
use pnpm_reporter::SilentReporter;
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

const FORCE_KEEP: ImportIndexedDirOpts =
    ImportIndexedDirOpts { force: true, keep_modules_dir: true, safe_to_skip: false };
const FORCE_ONLY: ImportIndexedDirOpts =
    ImportIndexedDirOpts { force: true, keep_modules_dir: false, safe_to_skip: false };
const FORCE_SHARED: ImportIndexedDirOpts =
    ImportIndexedDirOpts { force: true, keep_modules_dir: false, safe_to_skip: true };
// The shape the isolated linker uses for a shared slot: no force, since a warm slot is
// short-circuited by its marker before the import runs.
const SHARED: ImportIndexedDirOpts =
    ImportIndexedDirOpts { force: false, keep_modules_dir: false, safe_to_skip: true };
// The shape the isolated linker uses for a shared slot whose build was interrupted.
const FORCE_SHARED_KEEP: ImportIndexedDirOpts =
    ImportIndexedDirOpts { force: true, keep_modules_dir: true, safe_to_skip: true };

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
fn safe_to_skip_keeps_a_target_a_concurrent_importer_already_completed() {
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"1.0.0\"}");
    let index = write_source(&src_root, "index.js", b"module.exports = 1");
    let cas = cas_map(&[("package.json", pkg_json), ("index.js", index)]);

    let target = tmp.path().join("slot");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("package.json"), b"{\"version\":\"1.0.0\"}").unwrap();
    fs::write(target.join("index.js"), b"module.exports = 1").unwrap();

    let logged_methods = AtomicU8::new(0);
    import_indexed_dir::<SilentReporter>(
        &logged_methods,
        PackageImportMethod::Copy,
        &target,
        &cas,
        FORCE_SHARED,
    )
    .expect("a slot another importer already completed is not a conflict");

    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"{\"version\":\"1.0.0\"}");
    assert_eq!(fs::read(target.join("index.js")).unwrap(), b"module.exports = 1");
    assert_eq!(
        logged_methods.load(std::sync::atomic::Ordering::Relaxed),
        0,
        "an already matching shared slot must not be imported into a throwaway stage",
    );
    let strays: Vec<_> = fs::read_dir(tmp.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != "cas" && name != "slot")
        .collect();
    assert_eq!(strays, Vec::<String>::new(), "staging dir must be cleaned up");
}

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

// Both stacks let the exclusive mkdir, not the earlier stat, decide who imports a shared slot.
#[test]
fn claim_dir_reports_only_the_creating_call() {
    let tmp = tempdir().unwrap();
    let slot = tmp.path().join("nested").join("slot");

    assert!(claim_dir(&slot).unwrap(), "the call that creates the slot owns it");
    assert!(!claim_dir(&slot).unwrap(), "a later call finds it taken");
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

#[test]
fn concurrent_importers_of_one_shared_slot_both_succeed() {
    use std::sync::{Arc, Barrier};

    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"{\"version\":\"1.0.0\"}");
    let index = write_source(&src_root, "index.js", b"module.exports = 1");
    let cas = Arc::new(cas_map(&[("package.json", pkg_json), ("index.js", index)]));

    let target = Arc::new(tmp.path().join("slot"));
    let barrier = Arc::new(Barrier::new(2));

    let handles: Vec<_> = (0..2)
        .map(|_| {
            let cas = Arc::clone(&cas);
            let target = Arc::clone(&target);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                import_indexed_dir::<SilentReporter>(
                    &AtomicU8::new(0),
                    PackageImportMethod::Copy,
                    &target,
                    &cas,
                    FORCE_SHARED,
                )
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("importer thread panicked").expect("both importers must succeed");
    }

    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"{\"version\":\"1.0.0\"}");
    assert_eq!(fs::read(target.join("index.js")).unwrap(), b"module.exports = 1");
    let strays: Vec<String> = fs::read_dir(tmp.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != "cas" && name != "slot")
        .collect();
    assert_eq!(strays, Vec::<String>::new(), "neither importer may leak a staging dir");
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
