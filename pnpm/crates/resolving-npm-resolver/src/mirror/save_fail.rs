//! Test-only failure injection for [`crate::mirror::save_meta`].

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex, PoisonError},
};

use super::SaveMetaError;

/// Keyed by path rather than by thread: a fetch saves the mirror on the
/// blocking pool, and concurrent tests use their own temporary directories.
static FAIL_NEXT_MIRROR_SAVE: LazyLock<Mutex<HashSet<PathBuf>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// The next [`crate::mirror::save_meta`] of `pkg_mirror` returns an error
/// before touching disk.
pub(crate) fn fail_next_mirror_save(pkg_mirror: &Path) {
    failing_paths().insert(pkg_mirror.to_path_buf());
}

pub(super) fn forced_mirror_save_error(pkg_mirror: &Path) -> Option<SaveMetaError> {
    if !failing_paths().remove(pkg_mirror) {
        return None;
    }
    Some(SaveMetaError::WriteTemp {
        temp: pkg_mirror.to_path_buf(),
        error: std::io::Error::other("forced mirror save failure"),
    })
}

fn failing_paths() -> std::sync::MutexGuard<'static, HashSet<PathBuf>> {
    FAIL_NEXT_MIRROR_SAVE.lock().unwrap_or_else(PoisonError::into_inner)
}
