use crate::{FsRename, copy_dirent, cross_device::is_cross_device, remove_dirent};
use std::{fs, io, path::Path};

/// Move `src` onto `dst`, falling back to a copy when the kernel
/// refuses the rename as cross-device.
///
/// Two sibling paths can still be cross-device: overlayfs answers
/// `EXDEV` for a directory that is still on a lower layer, because
/// moving it means copying the whole subtree up first. A `pnpm fetch`
/// in an earlier Docker layer leaves exactly such a directory for the
/// install that follows to move. pnpm v11 handles the same signal in
/// `renameEvenAcrossDevices`.
///
/// The fallback copies `src` next to `dst` and renames that copy into
/// place, so both routes settle the destination through one rename and
/// report the same collisions. Copying straight onto `dst` would
/// instead merge into whatever is there, and a caller that handles the
/// collision itself would never hear about it.
///
/// `src` survives every failure: the staging copy is discarded, and a
/// removal that fails leaves both copies rather than risk being the
/// step that loses the data.
pub fn rename_even_across_devices<Sys: FsRename>(src: &Path, dst: &Path) -> io::Result<()> {
    match Sys::rename(src, dst) {
        Err(error) if is_cross_device(&error) => {}
        result => return result,
    }
    let parent = dst.parent().unwrap_or_else(|| Path::new("."));
    // Dropping the staging directory removes a copy left behind by a
    // failure, and the empty directory itself once the rename below has
    // taken the copy out of it.
    let staging = tempfile::Builder::new().prefix(".pnpm-cross-device-").tempdir_in(parent)?;
    let staged = staging.path().join("dirent");
    copy_dirent(src, &staged)?;
    // The copy is on the destination's own device, so this rename is
    // the one the caller asked for, refusals included.
    fs::rename(&staged, dst)?;
    remove_dirent(src)
}

#[cfg(test)]
mod tests;
