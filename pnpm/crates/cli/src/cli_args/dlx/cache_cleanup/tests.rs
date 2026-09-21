use super::clean_expired_dlx_cache;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

fn key_dir_with_link(cache_dir: &Path, key: &str) -> PathBuf {
    let key_dir = cache_dir.join("dlx").join(key);
    let target = key_dir.join("prepare-1");
    fs::create_dir_all(&target).unwrap();
    symlink_dir(&target, &key_dir.join("pkg")).unwrap();
    key_dir
}

#[test]
fn removes_expired_entries_and_keeps_fresh_ones() {
    let cache_dir = tempfile::tempdir().unwrap();
    let key_a = key_dir_with_link(cache_dir.path(), "key-a");
    let key_b = key_dir_with_link(cache_dir.path(), "key-b");

    let past_retention = SystemTime::now() + Duration::from_hours(48);
    clean_expired_dlx_cache(cache_dir.path(), 24 * 60, past_retention).unwrap();

    assert!(!key_a.exists(), "entry past the retention window should be removed");
    assert!(!key_b.exists(), "entry past the retention window should be removed");

    let key_a = key_dir_with_link(cache_dir.path(), "key-a");
    let key_b = key_dir_with_link(cache_dir.path(), "key-b");

    clean_expired_dlx_cache(cache_dir.path(), 24 * 60, SystemTime::now()).unwrap();

    assert!(key_a.exists(), "fresh link must survive within max age");
    assert!(key_b.exists(), "fresh link must survive within max age");
}

#[test]
fn zero_max_age_removes_everything_without_stat() {
    let cache_dir = tempfile::tempdir().unwrap();
    let key = key_dir_with_link(cache_dir.path(), "some-key");
    fs::write(
        cache_dir
            .path()
            .join("dlx")
            .join("stray-file"),
        b"noise",
    )
    .unwrap();

    clean_expired_dlx_cache(cache_dir.path(), 0, SystemTime::now()).unwrap();

    assert!(!key.exists(), "zero max age removes every entry");
    assert!(
        cache_dir
            .path()
            .join("dlx")
            .join("stray-file")
            .exists(),
        "files in the dlx directory are ignored",
    );
}

#[test]
fn removes_orphaned_prepare_dirs_but_keeps_the_link_target() {
    let cache_dir = tempfile::tempdir().unwrap();
    let key_dir = key_dir_with_link(cache_dir.path(), "key");
    let orphan = key_dir.join("prepare-orphan");
    fs::create_dir_all(&orphan).unwrap();
    let target = key_dir.join("prepare-1");

    clean_expired_dlx_cache(cache_dir.path(), u64::MAX / 60, SystemTime::now()).unwrap();

    assert!(!orphan.exists(), "orphaned prepare dir should be removed");
    assert!(target.exists(), "the link target must be kept");
    assert!(fs::symlink_metadata(key_dir.join("pkg")).is_ok(), "the pkg link itself must be kept");
}

#[test]
fn removes_key_dirs_left_behind_without_a_pkg_link() {
    let cache_dir = tempfile::tempdir().unwrap();
    let key_dir = cache_dir
        .path()
        .join("dlx")
        .join("linkless-key");
    fs::create_dir_all(key_dir.join("prepare-1")).unwrap();

    clean_expired_dlx_cache(cache_dir.path(), u64::MAX / 60, SystemTime::now()).unwrap();

    assert!(!key_dir.exists(), "key dir without a pkg link should be removed");
}

#[test]
fn missing_dlx_dir_is_not_an_error() {
    let cache_dir = tempfile::tempdir().unwrap();
    clean_expired_dlx_cache(cache_dir.path(), 0, SystemTime::now()).unwrap();
}

/// The live-target comparison must survive symlinked parents, e.g. the
/// `/var` -> `/private/var` alias macOS tempdirs sit behind: the `pkg`
/// link canonicalizes to the real path while directory entries report the
/// aliased one.
#[cfg(unix)]
#[test]
fn keeps_link_target_when_parent_is_symlinked() {
    use std::os::unix::fs::symlink;

    let real = tempfile::tempdir().unwrap();
    let cache_root = real.path().join("cache");
    fs::create_dir_all(&cache_root).unwrap();
    let alias = real.path().join("alias");
    symlink(&cache_root, &alias).unwrap();

    let key_dir = alias.join("dlx").join("key");
    let target = key_dir.join("prepare-1");
    fs::create_dir_all(&target).unwrap();
    symlink(&target, key_dir.join("pkg")).unwrap();
    let orphan = key_dir.join("prepare-orphan");
    fs::create_dir_all(&orphan).unwrap();

    clean_expired_dlx_cache(&alias, u64::MAX / 60, SystemTime::now()).unwrap();

    assert!(!orphan.exists(), "orphaned prepare dir should be removed");
    assert!(
        target.exists(),
        "the link target must be kept when reached through a symlinked parent",
    );
}

/// An orphaned symlink child must be unlinked itself; the sweep must not
/// follow it to a file outside the cache.
#[cfg(unix)]
#[test]
fn removes_orphan_symlink_without_touching_external_target() {
    use std::os::unix::fs::symlink;

    let cache_dir = tempfile::tempdir().unwrap();
    let key_dir = key_dir_with_link(cache_dir.path(), "key");
    let external = tempfile::tempdir().unwrap();
    let target = external.path().join("important.txt");
    fs::write(&target, b"do not delete").unwrap();
    let link = key_dir.join("prepare-stray-link");
    symlink(&target, &link).unwrap();

    clean_expired_dlx_cache(cache_dir.path(), u64::MAX / 60, SystemTime::now()).unwrap();

    assert!(fs::symlink_metadata(&link).is_err(), "orphaned symlink should be unlinked");
    assert!(target.exists(), "the file outside the cache must survive");
}
