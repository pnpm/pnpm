//! Records whose content change is a `GIT binary patch` block, as emitted
//! by `git diff --binary`.

use diffy::{
    binary::BinaryPatch,
    patch_set::{
        FileMode,
        FileOperation,
    },
};
use std::{
    fs,
    io,
    path::Path,
};

use super::{
    FileApply,
    PatchApplyError,
    atomic_write,
};

/// How a record's forward block relates to the file on disk.
enum BinaryDirection {
    /// The file matches the recorded pre-image; writing this content
    /// applies the patch.
    Forward(Vec<u8>),
    /// The file matches the recorded post-image; the record has already
    /// been applied.
    AlreadyPatched,
}

/// Decide which direction the record's blocks treat `bytes` as.
///
/// Decoding a block validates the direction it maps from, not that the
/// file is in the state the patch expects. A `literal` block ignores its
/// input entirely, and a `delta` block checks lengths and copy ranges
/// without confirming the bytes, so a plain forward success proves
/// nothing. A direction is only accepted when it round-trips: the block
/// must map `bytes` to the opposite image and back without changing it.
/// Re-applying an equal-length block-rearranging patch then resolves to
/// [`BinaryDirection::AlreadyPatched`] instead of corrupting the file.
fn resolve_binary_direction(
    binary_patch: &BinaryPatch<'_>,
    bytes: &[u8],
) -> Option<BinaryDirection> {
    if let Ok(post) = binary_patch.apply(bytes) {
        if binary_patch.apply_reverse(&post).as_deref() == Ok(bytes) {
            return Some(BinaryDirection::Forward(post));
        }
    }
    if let Ok(pre) = binary_patch.apply_reverse(bytes) {
        if binary_patch.apply(&pre).as_deref() == Ok(bytes) {
            return Some(BinaryDirection::AlreadyPatched);
        }
    }
    None
}

impl FileApply<'_> {
    /// Modify a file with a `GIT binary patch` block (`git diff --binary`).
    ///
    /// The record's blocks are resolved against the target's current bytes
    /// (see [`resolve_binary_direction`]). A file that matches neither the
    /// recorded pre-image nor the post-image is refused, and an
    /// already-patched file is left untouched, the same idempotency rule
    /// as the text [`Self::modify`], including the atomic re-write when a
    /// mode change is still pending or the target is a symlink.
    fn modify_binary(
        &self,
        target: &Path,
        binary_patch: &BinaryPatch<'_>,
        new_mode: Option<&FileMode>,
    ) -> Result<(), PatchApplyError> {
        let permissions = atomic_write::modify_permissions(target, new_mode)
            .map_err(|source| self.failed(format!("stat {}: {source}", target.display())))?;
        let bytes = fs::read(target)
            .map_err(|source| self.failed(format!("read {}: {source}", target.display())))?;
        let updated = match resolve_binary_direction(binary_patch, &bytes) {
            Some(BinaryDirection::Forward(updated)) => updated,
            Some(BinaryDirection::AlreadyPatched)
                if !atomic_write::needs_mode_change(target, new_mode) && !target.is_symlink() =>
            {
                return Ok(());
            }
            // Already patched, but a mode change is pending or the target
            // is a symlink: re-write the file's own (post-image) bytes.
            Some(BinaryDirection::AlreadyPatched) => bytes,
            None => {
                return Err(self.failed(format!(
                    "apply to {}: the file matches neither the record's pre-image nor its post-image",
                    target.display(),
                )));
            }
        };
        atomic_write::write_atomic_with_mode(target, &updated, Some(&permissions))
            .map_err(|source| self.failed(format!("write {}: {source}", target.display())))
    }

    /// Create a file with the content a `GIT binary patch` block carries:
    /// the forward block decoded against an empty preimage. The
    /// no-overwrite rule and the already-created idempotency exception
    /// mirror the text [`Self::create`].
    fn create_binary(
        &self,
        target: &Path,
        binary_patch: &BinaryPatch<'_>,
        new_mode: Option<&FileMode>,
    ) -> Result<(), PatchApplyError> {
        let created = binary_patch
            .apply(&[])
            .map_err(|error| self.failed(format!("create {}: {error}", target.display())))?;
        let exists = target.symlink_metadata().is_ok();
        if exists && self.existing_file_matches(target, &created, new_mode)? {
            return Ok(());
        }
        self.create_parent_dir(target)?;
        let permissions = atomic_write::create_permissions(new_mode);
        atomic_write::write_atomic_with_mode(target, &created, permissions.as_ref())
            .map_err(|source| self.failed(format!("write {}: {source}", target.display())))
    }

    /// Delete a file. The record's reverse block carries the pre-image, so
    /// the file on disk is checked against it before unlinking; a header
    /// without blocks (what `git diff --irreversible-delete` emits) is
    /// deleted on the header alone, as pnpm 11 does for every deletion.
    fn delete_binary(
        &self,
        target: &Path,
        binary_patch: &BinaryPatch<'_>,
    ) -> Result<(), PatchApplyError> {
        if let BinaryPatch::Full { .. } = binary_patch {
            let bytes = match fs::read(target) {
                Ok(bytes) => bytes,
                // Already removed by an earlier apply of the same patch.
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(source) => {
                    return Err(self.failed(format!("read {}: {source}", target.display())));
                }
            };
            let preimage = binary_patch
                .apply_reverse(&[])
                .map_err(|error| self.failed(format!("read {}: {error}", target.display())))?;
            if bytes != preimage {
                return Err(self.failed(format!(
                    "delete patch targets a file that does not match the recorded pre-image: {}",
                    target.display(),
                )));
            }
        }
        match fs::remove_file(target) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(self.failed(format!("delete {}: {source}", target.display()))),
        }
    }

    /// Move `from` to `to` and apply the record's content blocks, keeping
    /// the already-patched exception of [`Self::modify_binary`].
    fn rename_binary(
        &self,
        from: &Path,
        to: &Path,
        binary_patch: &BinaryPatch<'_>,
        new_mode: Option<&FileMode>,
    ) -> Result<(), PatchApplyError> {
        self.move_entry(from, to)?;
        if matches!(binary_patch, BinaryPatch::Marker)
            && !atomic_write::needs_mode_change(to, new_mode)
        {
            return Ok(());
        }
        self.modify_binary(to, binary_patch, new_mode)
    }

    /// Apply one binary record, dispatching on the operation.
    pub(super) fn binary_operation(
        &self,
        operation: &FileOperation<'_, str>,
        binary_patch: &BinaryPatch<'_>,
        new_mode: Option<&FileMode>,
    ) -> Result<(), PatchApplyError> {
        match operation {
            FileOperation::Modify { modified, .. } => self.modify_binary(
                &self.resolve_target(Path::new(modified.as_ref()))?,
                binary_patch,
                new_mode,
            ),
            FileOperation::Create(path) => self.create_binary(
                &self.resolve_target(Path::new(path.as_ref()))?,
                binary_patch,
                new_mode,
            ),
            FileOperation::Delete(path) => {
                self.delete_binary(&self.resolve_target(Path::new(path.as_ref()))?, binary_patch)
            }
            FileOperation::Rename { from, to } => self.rename_binary(
                &self.resolve_target(Path::new(from.as_ref()))?,
                &self.resolve_target(Path::new(to.as_ref()))?,
                binary_patch,
                new_mode,
            ),
            FileOperation::Copy { .. } => {
                Err(self.failed("copy operations in patches are not yet supported".to_string()))
            }
        }
    }

    /// Decode a binary record against manifest bytes for the preview. The
    /// preview must be able to see a build script a binary manifest patch
    /// adds, so this resolves the direction exactly like the real apply,
    /// keeping the already-patched exception. Manifest bytes pass through
    /// a lossy UTF-8 conversion because the preview chains records as text.
    pub(super) fn preview_binary_manifest(
        binary_patch: &BinaryPatch<'_>,
        original: &str,
    ) -> Result<String, String> {
        match resolve_binary_direction(binary_patch, original.as_bytes()) {
            Some(BinaryDirection::Forward(updated)) => {
                Ok(String::from_utf8_lossy(&updated).into_owned())
            }
            Some(BinaryDirection::AlreadyPatched) => Ok(original.to_string()),
            None => {
                Err("the file matches neither the record's pre-image nor its post-image".into())
            }
        }
    }
}
