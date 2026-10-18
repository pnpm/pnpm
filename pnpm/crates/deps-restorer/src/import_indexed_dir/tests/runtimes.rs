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
/// A stale subtree the importer cannot unlink does not block the
/// replacement: the staged tree takes the target's place with the
/// preserved `node_modules/`, and only discarding the old tree fails,
/// which leaves it next to the target and warns the user to remove it.
///
/// `remove_dir_all` fails on a subdirectory chmod'ed to 0o500: it can
/// read the entries but unlinking them needs write on the containing
/// directory.
#[test]
#[cfg(unix)]
fn undeletable_stale_subtree_does_not_block_the_forced_replacement() {
    use pnpm_reporter::{LogEvent, Reporter};
    use std::{os::unix::fs::PermissionsExt, sync::Mutex};

    static WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            if let LogEvent::Global(log) = event {
                WARNINGS
                    .lock()
                    .unwrap()
                    .push(log.message.clone());
            }
        }
    }

    WARNINGS.lock().unwrap().clear();
    let tmp = tempdir().unwrap();
    let src_root = tmp.path().join("cas");
    fs::create_dir_all(&src_root).unwrap();
    let pkg_json = write_source(&src_root, "package.json", b"new");
    let bundled = write_source(&src_root, "bundled.js", b"bundled");
    let cas = cas_map(&[("package.json", pkg_json), ("node_modules/inner/bundled.js", bundled)]);

    let target = tmp.path().join("pkg");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("stale.txt"), b"stale").unwrap();
    fs::create_dir_all(target.join("node_modules/kept")).unwrap();
    fs::write(target.join("node_modules/kept/sentinel"), b"survivor").unwrap();
    let locked = target.join("locked_subdir");
    fs::create_dir_all(&locked).unwrap();
    fs::write(locked.join("immutable.txt"), b"locked").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();

    let result = import_indexed_dir::<RecordingReporter>(
        &AtomicU8::new(0),
        PackageImportMethod::Copy,
        &target,
        &cas,
        FORCE_KEEP,
    );

    let displaced: Vec<_> = fs::read_dir(tmp.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains("pacquet-stage"))
        })
        .collect();
    // Restore perms so the tempdir teardown can succeed.
    for path in &displaced {
        let locked = path.join("locked_subdir");
        if locked.exists() {
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
        }
    }

    result.expect("the replacement must succeed");
    assert_eq!(fs::read(target.join("package.json")).unwrap(), b"new");
    assert_eq!(fs::read(target.join("node_modules/kept/sentinel")).unwrap(), b"survivor");
    assert_eq!(fs::read(target.join("node_modules/inner/bundled.js")).unwrap(), b"bundled");
    assert!(!target.join("stale.txt").exists());
    assert!(!target.join("locked_subdir").exists());
    assert_eq!(displaced.len(), 1, "the old tree is left aside: {displaced:?}");
    assert_eq!(fs::read(displaced[0].join("locked_subdir/immutable.txt")).unwrap(), b"locked");
    let warnings = WARNINGS.lock().unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains(&displaced[0].to_string_lossy().into_owned()), "{}", warnings[0]);
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
            !path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.contains("pacquet-stage")),
            "staging directory leaked at {path:?}",
        );
    }
}

fn cross_device_error() -> std::io::Error {
    #[cfg(unix)]
    return std::io::Error::from_raw_os_error(18);
    #[cfg(windows)]
    return std::io::Error::from_raw_os_error(17);
}

/// A filesystem provider that reports `EXDEV` on rename, simulating
/// overlayfs when moving a directory from an inherited image layer.
struct CrossDevice;

impl pnpm_fs::FsRename for CrossDevice {
    fn rename(_src: &std::path::Path, _dst: &std::path::Path) -> std::io::Result<()> {
        Err(cross_device_error())
    }
}

impl pnpm_fs::FsRemoveDirent for CrossDevice {
    fn remove_dirent(path: &std::path::Path) -> std::io::Result<()> {
        pnpm_fs::remove_dirent(path)
    }
}

#[test]
fn preserve_modules_dir_cross_device_fallback_copies_and_removes_source() {
    let tmp = tempdir().unwrap();
    let source = tmp.path().join("source_node_modules");
    fs::create_dir_all(source.join("inner")).unwrap();
    fs::write(source.join("inner/index.js"), b"// inner dep").unwrap();
    fs::write(source.join("file.js"), b"// preserved dep").unwrap();

    let destination = tmp.path().join("staged_node_modules");
    let backup = tmp.path().join("backup_node_modules");

    let preserved =
        super::super::staging::preserve_modules_dir::<CrossDevice>(&source, &destination, &backup)
            .expect("cross-device rename should fall back to copy and delete");

    assert_eq!(preserved, super::super::PreservedModules::Directory);
    assert_eq!(fs::read(destination.join("file.js")).unwrap(), b"// preserved dep");
    assert_eq!(fs::read(destination.join("inner/index.js")).unwrap(), b"// inner dep");
    assert!(!source.exists(), "source must be removed after move");
    assert!(!backup.exists(), "backup must not be left behind on direct move");
}

#[test]
fn preserve_modules_dir_cross_device_with_collision_merges() {
    let tmp = tempdir().unwrap();
    let source = tmp.path().join("source_node_modules");
    fs::create_dir_all(source.join("existing")).unwrap();
    fs::write(source.join("existing/keep.js"), b"survivor").unwrap();
    fs::create_dir_all(source.join("foo")).unwrap();
    fs::write(source.join("foo/stale.js"), b"replaced").unwrap();

    let destination = tmp.path().join("staged_node_modules");
    fs::create_dir_all(destination.join("foo")).unwrap();
    fs::write(destination.join("foo/index.js"), b"bundled").unwrap();

    let backup = tmp.path().join("backup_node_modules");

    let preserved =
        super::super::staging::preserve_modules_dir::<CrossDevice>(&source, &destination, &backup)
            .expect("cross-device collision should merge via backup");

    assert!(matches!(preserved, super::super::PreservedModules::Merged { .. }));
    assert_eq!(fs::read(destination.join("existing/keep.js")).unwrap(), b"survivor");
    assert_eq!(fs::read(destination.join("foo/index.js")).unwrap(), b"bundled");
    assert!(
        !destination.join("foo/stale.js").exists(),
        "conflicting entries in source must not overwrite destination",
    );
    assert!(!source.exists(), "source must be removed after merge");
}

#[test]
fn restore_preserved_node_modules_cross_device() {
    let tmp = tempdir().unwrap();
    let staged = tmp.path().join("staged_node_modules");
    fs::create_dir_all(staged.join("inner")).unwrap();
    fs::write(staged.join("inner/index.js"), b"// inner dep").unwrap();

    let target = tmp.path().join("target_node_modules");
    let preserved = super::super::PreservedModules::Directory;

    let restored = super::super::staging::restore_preserved_node_modules::<CrossDevice>(
        &preserved, &staged, &target,
    );
    assert!(restored);
    assert_eq!(fs::read(target.join("inner/index.js")).unwrap(), b"// inner dep");
    assert!(!staged.exists(), "staged copy must be removed after restore");
}
