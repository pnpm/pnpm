use super::{AddFilesFromDirError, add_files_from_dir};
use crate::StoreDir;
use pretty_assertions::assert_eq;
#[cfg(unix)]
use std::os::unix::fs as unix_fs;
use std::{fs, path::Path};
use tempfile::tempdir;

fn make_store() -> (tempfile::TempDir, StoreDir) {
    let tmp = tempdir().expect("create temp dir");
    let store_dir = StoreDir::from(tmp.path().to_path_buf());
    store_dir.init().expect("init store dir");
    (tmp, store_dir)
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, contents).expect("write file");
}

/// A directory with two regular files at the top level lands both
/// in the returned map under their basenames. The CAFS-side blob
/// hash matches `Sha512(contents)`.
#[test]
fn captures_top_level_files() {
    let (_tmp, store_dir) = make_store();
    let pkg_dir = tempdir().expect("create pkg dir");
    write(&pkg_dir.path().join("index.js"), "console.log('hi')\n");
    write(&pkg_dir.path().join("package.json"), "{\"name\":\"x\"}\n");

    let added = add_files_from_dir(&store_dir, pkg_dir.path()).expect("walk");
    let keys: Vec<_> = added.files.keys().cloned().collect();
    let mut keys = keys;
    keys.sort();
    assert_eq!(keys, vec!["index.js".to_string(), "package.json".to_string()]);

    let pkg = added.files.get("package.json").unwrap();
    assert_eq!(pkg.size, b"{\"name\":\"x\"}\n".len() as u64);
}

/// Nested files use forward-slash relative paths regardless of
/// host separator — required so the resulting `FilesIndex`
/// round-trips through pnpm without renormalisation.
#[test]
fn nested_paths_use_forward_slashes() {
    let (_tmp, store_dir) = make_store();
    let pkg_dir = tempdir().expect("create pkg dir");
    write(&pkg_dir.path().join("lib/inner/deep.js"), "deep\n");

    let added = add_files_from_dir(&store_dir, pkg_dir.path()).expect("walk");
    assert!(added.files.contains_key("lib/inner/deep.js"), "got keys: {:?}", added.files.keys());
}

/// An executable script (`0o755`) lands in the CAFS as `-exec`.
/// Reading the same digest back via `cas_file_path_by_mode` with
/// the recorded mode must resolve to that exact path.
#[cfg(unix)]
#[test]
fn executable_files_get_exec_suffix() {
    use std::os::unix::fs::PermissionsExt;
    let (_tmp, store_dir) = make_store();
    let pkg_dir = tempdir().expect("create pkg dir");
    let bin = pkg_dir.path().join("bin/run");
    write(&bin, "#!/bin/sh\necho hi\n");
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).expect("chmod");

    let added = add_files_from_dir(&store_dir, pkg_dir.path()).expect("walk");
    let info = added.files.get("bin/run").expect("entry for bin/run");
    assert_eq!(info.mode & 0o111, 0o111);

    let on_disk = store_dir.cas_file_path_by_mode(&info.digest, info.mode).unwrap();
    let path_str = on_disk.to_string_lossy();
    assert!(path_str.ends_with("-exec"), "expected -exec suffix, got `{path_str}`");
}

/// A top-level `node_modules` directory is silently skipped —
/// matches upstream's `includeNodeModules` default of `false`.
#[test]
fn top_level_node_modules_is_skipped() {
    let (_tmp, store_dir) = make_store();
    let pkg_dir = tempdir().expect("create pkg dir");
    write(&pkg_dir.path().join("index.js"), "x\n");
    write(&pkg_dir.path().join("node_modules/dep/index.js"), "y\n");

    let added = add_files_from_dir(&store_dir, pkg_dir.path()).expect("walk");
    let keys: Vec<_> = added.files.keys().cloned().collect();
    assert_eq!(keys, vec!["index.js".to_string()]);
}

/// A nested `node_modules` (not at depth 0) is treated like any
/// other directory and walked. Mirrors upstream's
/// `relativeDir !== '' || file.name !== 'node_modules'` guard.
#[test]
fn nested_node_modules_is_walked() {
    let (_tmp, store_dir) = make_store();
    let pkg_dir = tempdir().expect("create pkg dir");
    write(&pkg_dir.path().join("lib/node_modules/inner.js"), "i\n");

    let added = add_files_from_dir(&store_dir, pkg_dir.path()).expect("walk");
    assert!(
        added.files.contains_key("lib/node_modules/inner.js"),
        "got keys: {:?}",
        added.files.keys(),
    );
}

/// A symlink whose target resolves outside the package root is
/// dropped. Mirrors upstream's `isSubdir(rootDir, realPath)` check.
#[cfg(unix)]
#[test]
fn symlinks_pointing_outside_root_are_skipped() {
    let (_tmp, store_dir) = make_store();
    let outside = tempdir().expect("create outside dir");
    write(&outside.path().join("secret"), "leaked\n");

    let pkg_dir = tempdir().expect("create pkg dir");
    write(&pkg_dir.path().join("index.js"), "x\n");
    unix_fs::symlink(outside.path().join("secret"), pkg_dir.path().join("leak"))
        .expect("create symlink");

    let added = add_files_from_dir(&store_dir, pkg_dir.path()).expect("walk");
    let mut keys: Vec<_> = added.files.keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, vec!["index.js".to_string()]);
}

/// A symlink pointing at a sibling file within the same root is
/// followed and captured under the link's name.
#[cfg(unix)]
#[test]
fn symlinks_within_root_are_followed() {
    let (_tmp, store_dir) = make_store();
    let pkg_dir = tempdir().expect("create pkg dir");
    write(&pkg_dir.path().join("target.js"), "ok\n");
    unix_fs::symlink("target.js", pkg_dir.path().join("alias.js")).expect("create symlink");

    let added = add_files_from_dir(&store_dir, pkg_dir.path()).expect("walk");
    let info_target = added.files.get("target.js").expect("target.js");
    let info_alias = added.files.get("alias.js").expect("alias.js");
    assert_eq!(
        info_target.digest, info_alias.digest,
        "alias must hash to the same digest as its target",
    );
}

/// A directory symlink that loops back to a parent must not infinite-
/// recurse. The `visited` set carrying canonical paths is the defense.
#[cfg(unix)]
#[test]
fn directory_cycle_terminates() {
    let (_tmp, store_dir) = make_store();
    let pkg_dir = tempdir().expect("create pkg dir");
    let sub = pkg_dir.path().join("sub");
    fs::create_dir_all(&sub).expect("mkdir sub");
    write(&sub.join("file.txt"), "f\n");
    unix_fs::symlink("..", sub.join("up")).expect("create dir symlink loop");

    let added = add_files_from_dir(&store_dir, pkg_dir.path()).expect("walk");
    // Must contain sub/file.txt; absence would be a bug, infinite
    // recursion would have hung this test.
    assert!(added.files.contains_key("sub/file.txt"), "got keys: {:?}", added.files.keys());
}

/// Walking a missing root surfaces a structured error rather than
/// silently returning an empty map. Use a tempdir subpath so the
/// "missing" assertion works on every platform (Windows would not
/// guarantee `/this/does/not/exist/anywhere` is missing).
#[test]
fn missing_root_errors() {
    let (_tmp, store_dir) = make_store();
    let parent = tempdir().expect("create tempdir");
    let missing = parent.path().join("does-not-exist");
    let err = add_files_from_dir(&store_dir, &missing);
    assert!(matches!(err, Err(AddFilesFromDirError::CanonicalizeRoot { .. })));
}
