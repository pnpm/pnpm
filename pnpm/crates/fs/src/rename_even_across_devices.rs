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
/// Every failure leaves `src` where it was, and the copy is discarded
/// with the staging directory. Once the copy reaches `dst` the move has
/// happened, so a source that then refuses to be removed is reported as
/// a warning rather than an error: telling the caller the move failed
/// would send it looking for the data at a path that no longer holds
/// it.
pub fn rename_even_across_devices<Sys: FsRename>(src: &Path, dst: &Path) -> io::Result<()> {
    match Sys::rename(src, dst) {
        Err(error) if is_cross_device(&error) => {}
        result => return result,
    }
    if let Some(collision) = occupied_directory_collision(src, dst) {
        return Err(collision);
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
    if let Err(error) = remove_dirent(src) {
        tracing::warn!(
            target: "pacquet::rename_even_across_devices",
            ?src,
            ?dst,
            %error,
            "moved a directory across devices but could not remove the original",
        );
    }
    Ok(())
}

/// The error a rename onto an occupied directory would report, when
/// `dst` is one.
///
/// The copy below would otherwise materialize the whole of `src` only
/// for the rename to refuse the destination and the caller to move
/// `src` somewhere else instead — two full copies of a tree that can be
/// a package's entire dependency set.
fn occupied_directory_collision(src: &Path, dst: &Path) -> Option<io::Error> {
    if !fs::symlink_metadata(src).is_ok_and(|meta| meta.is_dir()) {
        return None;
    }
    if !fs::symlink_metadata(dst).is_ok_and(|meta| meta.is_dir()) {
        return None;
    }
    fs::read_dir(dst).ok()?.next().map(|_| io::Error::from(io::ErrorKind::DirectoryNotEmpty))
}

#[cfg(test)]
mod tests;
