//! Records whose content change is a `GIT binary patch` block, as emitted
//! by `git diff --binary`.

mod record;

use diffy::{
    binary::BinaryPatch,
    patch_set::{
        FileMode,
        FileOperation,
    },
};
use record::{
    BinaryDirection,
    BinaryRecord,
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

impl FileApply<'_> {
    /// Modify a file with a binary record.
    ///
    /// A file that is neither the recorded pre-image nor the post-image is
    /// refused. An already-patched file follows the idempotency rule of
    /// the text [`Self::modify`].
    fn modify_binary(
        &self,
        target: &Path,
        record: &BinaryRecord<'_, '_>,
        new_mode: Option<&FileMode>,
    ) -> Result<(), PatchApplyError> {
        let permissions = atomic_write::modify_permissions(target, new_mode)
            .map_err(|source| self.failed(format!("stat {}: {source}", target.display())))?;
        let bytes = fs::read(target)
            .map_err(|source| self.failed(format!("read {}: {source}", target.display())))?;
        let direction = record
            .direction(&bytes)
            .map_err(|message| self.failed(format!("apply to {}: {message}", target.display())))?;
        let updated = match direction {
            BinaryDirection::Forward(updated) => updated,
            BinaryDirection::AlreadyPatched
                if !atomic_write::needs_mode_change(target, new_mode) && !target.is_symlink() =>
            {
                return Ok(());
            }
            BinaryDirection::AlreadyPatched => bytes,
        };
        atomic_write::write_atomic_with_mode(target, &updated, Some(&permissions))
            .map_err(|source| self.failed(format!("write {}: {source}", target.display())))
    }

    /// Create a file with the content of a binary record. The no-overwrite
    /// rule and the already-created exception mirror the text
    /// [`Self::create`].
    fn create_binary(
        &self,
        target: &Path,
        record: &BinaryRecord<'_, '_>,
        new_mode: Option<&FileMode>,
    ) -> Result<(), PatchApplyError> {
        let created = record
            .created()
            .map_err(|message| self.failed(format!("create {}: {message}", target.display())))?;
        let exists = target.symlink_metadata().is_ok();
        if exists && self.existing_file_matches(target, &created, new_mode)? {
            return Ok(());
        }
        self.create_parent_dir(target)?;
        let permissions = atomic_write::create_permissions(new_mode);
        atomic_write::write_atomic_with_mode(target, &created, permissions.as_ref())
            .map_err(|source| self.failed(format!("write {}: {source}", target.display())))
    }

    /// Delete a file after checking it is the recorded pre-image. A missing
    /// file was removed by an earlier apply of the same patch.
    fn delete_binary(
        &self,
        target: &Path,
        record: &BinaryRecord<'_, '_>,
    ) -> Result<(), PatchApplyError> {
        let bytes = match fs::read(target) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(self.failed(format!("read {}: {source}", target.display())));
            }
        };
        if !record.is_preimage(&bytes) {
            return Err(self.failed(format!(
                "delete patch targets a file that does not match the recorded pre-image: {}",
                target.display(),
            )));
        }
        self.remove_file(target)
    }

    /// Delete a file on the header alone, for a `Binary files ... differ`
    /// record, as pnpm 11 does for every deletion.
    fn remove_file(&self, target: &Path) -> Result<(), PatchApplyError> {
        match fs::remove_file(target) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(self.failed(format!("delete {}: {source}", target.display()))),
        }
    }

    /// Move `from` to `to` and apply the record there. A `from` that is
    /// neither recorded image is refused before anything moves.
    fn rename_binary(
        &self,
        from: &Path,
        to: &Path,
        record: &BinaryRecord<'_, '_>,
        new_mode: Option<&FileMode>,
    ) -> Result<(), PatchApplyError> {
        if from.symlink_metadata().is_ok() {
            let bytes = fs::read(from)
                .map_err(|source| self.failed(format!("read {}: {source}", from.display())))?;
            if !record.is_recorded_image(&bytes) {
                return Err(self.failed(format!(
                    "rename {}: the file matches neither the record's pre-image nor its post-image",
                    from.display(),
                )));
            }
        }
        self.move_entry(from, to)?;
        self.modify_binary(to, record, new_mode)
    }

    /// Apply one binary record, dispatching on the operation.
    pub(super) fn binary_operation(
        &self,
        operation: &FileOperation<'_, str>,
        binary_patch: &BinaryPatch<'_>,
        new_mode: Option<&FileMode>,
    ) -> Result<(), PatchApplyError> {
        if let (BinaryPatch::Marker, FileOperation::Delete(path)) = (binary_patch, operation) {
            return self.remove_file(&self.resolve_target(Path::new(path.as_ref()))?);
        }
        let record = BinaryRecord::new(self.patch_text, binary_patch)
            .map_err(|message| self.failed(format!("{}: {message}", operation_path(operation))))?;
        match operation {
            FileOperation::Modify { modified, .. } => self.modify_binary(
                &self.resolve_target(Path::new(modified.as_ref()))?,
                &record,
                new_mode,
            ),
            FileOperation::Create(path) => self.create_binary(
                &self.resolve_target(Path::new(path.as_ref()))?,
                &record,
                new_mode,
            ),
            FileOperation::Delete(path) => {
                self.delete_binary(&self.resolve_target(Path::new(path.as_ref()))?, &record)
            }
            FileOperation::Rename { from, to } => self.rename_binary(
                &self.resolve_target(Path::new(from.as_ref()))?,
                &self.resolve_target(Path::new(to.as_ref()))?,
                &record,
                new_mode,
            ),
            FileOperation::Copy { .. } => {
                Err(self.failed("copy operations in patches are not yet supported".to_string()))
            }
        }
    }
}

/// Resolve a binary record against the manifest for the preview, the
/// way [`FileApply::create_binary`] and [`FileApply::modify_binary`]
/// resolve it.
pub(super) fn preview_binary_manifest(
    patch_text: &str,
    operation: &FileOperation<'_, str>,
    binary_patch: &BinaryPatch<'_>,
    original: String,
) -> Result<String, String> {
    let record = BinaryRecord::new(patch_text, binary_patch)?;
    let updated = match operation {
        FileOperation::Create(_) => record.created()?,
        _ => match record.direction(original.as_bytes())? {
            BinaryDirection::Forward(updated) => updated,
            BinaryDirection::AlreadyPatched => return Ok(original),
        },
    };
    Ok(String::from_utf8_lossy(&updated).into_owned())
}

/// The path a record names, for error messages.
fn operation_path<'operation>(operation: &'operation FileOperation<'_, str>) -> &'operation str {
    match operation {
        FileOperation::Modify { modified: path, .. }
        | FileOperation::Create(path)
        | FileOperation::Delete(path)
        | FileOperation::Rename { to: path, .. }
        | FileOperation::Copy { to: path, .. } => path.as_ref(),
    }
}
