//! Test-only failure injection for [`crate::mirror::save_meta`].

use std::{cell::Cell, path::Path};

use super::SaveMetaError;

thread_local! {
    static FAIL_NEXT_MIRROR_SAVE: Cell<bool> = const { Cell::new(false) };
}

/// The next [`crate::mirror::save_meta`] on this thread returns an error
/// before touching disk.
pub(crate) fn fail_next_mirror_save() {
    FAIL_NEXT_MIRROR_SAVE.with(|fail| fail.set(true));
}

pub(super) fn forced_mirror_save_error(pkg_mirror: &Path) -> Option<SaveMetaError> {
    if !take_fail_flag() {
        return None;
    }
    Some(SaveMetaError::WriteTemp {
        temp: pkg_mirror.to_path_buf(),
        error: std::io::Error::other("forced mirror save failure"),
    })
}

fn take_fail_flag() -> bool {
    FAIL_NEXT_MIRROR_SAVE.with(|fail| {
        let armed = fail.get();
        fail.set(false);
        armed
    })
}
