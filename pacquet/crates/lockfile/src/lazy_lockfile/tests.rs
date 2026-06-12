use super::{LazyLockfile, MaybeLazyLockfile};
use crate::Lockfile;

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
    let lazy = LazyLockfile::deferred(false);
    assert!(lazy.get().expect("disabled load is infallible").is_none());
    assert!(!lazy.is_loaded_or_on_disk());
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
