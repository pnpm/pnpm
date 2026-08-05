use super::{LazyLockfile, MaybeLazyLockfile};
use crate::Lockfile;
use std::fs;

fn minimal_lockfile() -> Lockfile {
    serde_saphyr::from_str("lockfileVersion: '9.0'\n").expect("parse a minimal lockfile")
}

#[test]
fn preloaded_returns_the_stored_lockfile_without_io() {
    let lazy = LazyLockfile::preloaded(Some(minimal_lockfile()));
    let loaded = lazy.get().expect("preloaded lockfile loads infallibly");
    assert!(loaded.is_some());
    assert!(lazy.is_loaded_or_on_disk());
}

#[test]
fn preloaded_none_reports_absent() {
    let lazy = LazyLockfile::preloaded(None);
    assert!(lazy.get().expect("preloaded lockfile loads infallibly").is_none());
    assert!(!lazy.is_loaded_or_on_disk());
}

#[test]
fn disabled_never_touches_the_filesystem() {
    let lazy = LazyLockfile::disabled();
    assert!(lazy.get().expect("disabled load is infallible").is_none());
    assert!(!lazy.is_loaded_or_on_disk());
}

#[test]
fn deferred_loads_from_the_given_dir_not_the_process_cwd() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join(Lockfile::FILE_NAME), "lockfileVersion: '9.0'\n")
        .expect("write pnpm-lock.yaml");

    let lazy = LazyLockfile::deferred(dir.path().to_path_buf());
    assert!(lazy.is_loaded_or_on_disk(), "probe must find the dir-addressed lockfile");
    assert!(lazy.get().expect("deferred load succeeds").is_some());

    let empty = tempfile::tempdir().expect("tempdir");
    let lazy = LazyLockfile::deferred(empty.path().to_path_buf());
    assert!(!lazy.is_loaded_or_on_disk());
    assert!(lazy.get().expect("absent lockfile loads as None").is_none());
}

#[test]
fn empty_and_env_only_files_count_as_absent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(Lockfile::FILE_NAME);

    fs::write(&path, "").expect("write empty lockfile");
    let lazy = LazyLockfile::deferred(dir.path().to_path_buf());
    assert!(!lazy.is_loaded_or_on_disk(), "an empty file must count as absent");

    fs::write(&path, "---\nenvDependencies:\n  node: '22.0.0'\n").expect("write env-only lockfile");
    let lazy = LazyLockfile::deferred(dir.path().to_path_buf());
    assert!(!lazy.is_loaded_or_on_disk(), "an env-only document must count as absent");
    assert!(lazy.get().expect("env-only lockfile loads as None").is_none());
}

#[cfg(unix)]
#[test]
fn unreadable_lockfile_counts_as_present() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(Lockfile::FILE_NAME);
    fs::write(&path, "lockfileVersion: '9.0'\n").expect("write pnpm-lock.yaml");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).expect("drop permissions");

    let lazy = LazyLockfile::deferred(dir.path().to_path_buf());
    let present = lazy.is_loaded_or_on_disk();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("restore permissions");
    assert!(present, "an unreadable lockfile must not be mistaken for a missing one");
}

#[test]
fn loaded_variant_passes_through() {
    let lockfile = minimal_lockfile();
    let maybe = MaybeLazyLockfile::Loaded(Some(&lockfile));
    assert!(maybe.get().expect("loaded variant is infallible").is_some());
    assert!(maybe.is_loaded_or_on_disk());
    let maybe = MaybeLazyLockfile::Loaded(None);
    assert!(maybe.get().expect("loaded variant is infallible").is_none());
    assert!(!maybe.is_loaded_or_on_disk());
}
