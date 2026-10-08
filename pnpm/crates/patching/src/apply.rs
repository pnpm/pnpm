pub use preview::{MANIFEST_FILE_NAME, PatchPreview, preview_patch};

mod atomic_write;
mod preview;
mod tolerant;

use atomic_write::write_atomic_with_mode;
use derive_more::{Display, Error};
use diffy::{
    Patch,
    patch_set::{FileMode, FileOperation, FilePatch, ParseOptions, PatchSet},
};
use miette::Diagnostic;
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

/// Error from [`apply_patch_to_dir`].
///
/// Diagnostic codes:
///
/// - `ERR_PNPM_PATCH_NOT_FOUND` — the patch file is missing.
/// - `ERR_PNPM_INVALID_PATCH` — the patch file can't be parsed.
/// - `ERR_PNPM_PATCH_FAILED` — a hunk wouldn't apply, the target
///   file is missing, or an IO error hit a target path.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum PatchApplyError {
    #[display("Patch file not found: {}", path.display())]
    #[diagnostic(code(ERR_PNPM_PATCH_NOT_FOUND))]
    PatchNotFound { path: PathBuf },

    #[display("Failed to read patch file {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_PATCH_NOT_FOUND))]
    ReadPatchFile {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Applying patch \"{}\" failed: {message}", patch_file_path.display())]
    #[diagnostic(code(ERR_PNPM_INVALID_PATCH))]
    InvalidPatch {
        patch_file_path: PathBuf,
        #[error(not(source))]
        message: String,
    },

    #[display("Could not apply patch {} to {}: {message}", patch_file_path.display(), patched_dir.display())]
    #[diagnostic(code(ERR_PNPM_PATCH_FAILED))]
    PatchFailed {
        patch_file_path: PathBuf,
        patched_dir: PathBuf,
        #[error(not(source))]
        message: String,
    },
}

/// Apply a unified-diff patch file to every modified/created/deleted
/// file inside `patched_dir`.
///
/// Uses [`diffy`] for parsing and applying — pure Rust, no subprocess,
/// no Node, cross-platform. Running `patch` or `git apply` directly
/// would be simpler, but `patch` is not available on Windows and
/// `git apply` is hard to execute on a subdirectory of an existing
/// repository, so an in-process applier sidesteps both problems.
///
/// Supported file operations: `Modify`, `Create`, `Delete`.
///
/// File paths in the patch are stripped one level
/// (`diffy::FileOperation::strip_prefix(1)`) to drop the conventional
/// `a/` and `b/` prefixes git uses, then validated against
/// `patched_dir`: absolute paths, `..` segments, and (on Windows)
/// drive prefixes / root components are rejected as
/// `ERR_PNPM_PATCH_FAILED`.
pub fn apply_patch_to_dir(
    patched_dir: &Path,
    patch_file_path: &Path,
) -> Result<(), PatchApplyError> {
    let canonical_dir = fs::canonicalize(patched_dir)
        .map_err(|source| PatchApplyError::PatchFailed {
            patch_file_path: patch_file_path.to_path_buf(),
            patched_dir: patched_dir.to_path_buf(),
            message: format!("canonicalize {}: {source}", patched_dir.display()),
        })?;
    let text = read_patch_text(patch_file_path)?;

    let patches = PatchSet::parse(&text, ParseOptions::gitdiff());
    for file_patch_result in patches {
        let file_patch = file_patch_result.map_err(|source| PatchApplyError::InvalidPatch {
            patch_file_path: patch_file_path.to_path_buf(),
            message: source.to_string(),
        })?;
        apply_one_file(patched_dir, &canonical_dir, patch_file_path, &file_patch)?;
    }
    Ok(())
}

/// Read a patch file and apply the tolerances that let the patch files
/// pnpm meets in the wild through [`PatchSet::parse`].
///
/// [`tolerant`] owns the tolerances; this is only the order it expects
/// them in, kept in one place so the apply and preview paths cannot drift.
pub(super) fn read_patch_text(patch_file_path: &Path) -> Result<String, PatchApplyError> {
    let text = read_patch_file(patch_file_path)?;
    Ok(tolerant::drop_context_no_newline_markers(tolerant::strip_cr_from_extended_headers(text)))
}

/// Read a patch file, mapping a missing file to
/// `ERR_PNPM_PATCH_NOT_FOUND`.
///
/// Decoded lossily to match Node `fs.readFile(path, 'utf8')` (the same
/// decoding [`create_hex_hash_from_file`] uses), so a patch file with
/// stray bytes still parses.
///
/// [`create_hex_hash_from_file`]: crate::create_hex_hash_from_file
fn read_patch_file(patch_file_path: &Path) -> Result<String, PatchApplyError> {
    match fs::read(patch_file_path) {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Err(PatchApplyError::PatchNotFound { path: patch_file_path.to_path_buf() })
        }
        Err(source) => {
            Err(PatchApplyError::ReadPatchFile { path: patch_file_path.to_path_buf(), source })
        }
    }
}

fn apply_one_file(
    patched_dir: &Path,
    canonical_dir: &Path,
    patch_file_path: &Path,
    file_patch: &FilePatch<'_, str>,
) -> Result<(), PatchApplyError> {
    let operation = file_patch.operation().strip_prefix(1);
    let apply = FileApply { patched_dir, canonical_dir, patch_file_path };

    let text_patch = || {
        file_patch
            .patch()
            .as_text()
            .ok_or_else(|| apply.failed("binary patch is not supported".to_string()))
    };
    match operation {
        FileOperation::Modify { modified, .. } => apply.modify(
            &apply.resolve_target(Path::new(modified.as_ref()))?,
            text_patch()?,
            file_patch.new_mode(),
        ),
        FileOperation::Create(path) => apply.create(
            &apply.resolve_target(Path::new(path.as_ref()))?,
            text_patch()?,
            file_patch.new_mode(),
        ),
        FileOperation::Delete(path) => {
            apply.delete(&apply.resolve_target(Path::new(path.as_ref()))?, text_patch()?)
        }
        FileOperation::Rename { .. } | FileOperation::Copy { .. } => {
            Err(apply.failed("rename/copy operations in patches are not yet supported".to_string()))
        }
    }
}

/// One patch record being applied to one file of `patched_dir`.
struct FileApply<'a> {
    patched_dir: &'a Path,
    canonical_dir: &'a Path,
    patch_file_path: &'a Path,
}

impl FileApply<'_> {
    fn failed(&self, message: String) -> PatchApplyError {
        PatchApplyError::PatchFailed {
            patch_file_path: self.patch_file_path.to_path_buf(),
            patched_dir: self.patched_dir.to_path_buf(),
            message,
        }
    }

    /// Reject patch paths that try to escape `patched_dir`: absolute paths,
    /// `..` segments, drive-letter prefixes, root components, or intermediate
    /// directory symlinks traversing outside `patched_dir`.
    fn resolve_target(&self, rel: &Path) -> Result<PathBuf, PatchApplyError> {
        let escapes = rel.is_absolute()
            || rel
                .components()
                .any(|component| {
                    matches!(
                        component,
                        Component::ParentDir | Component::RootDir | Component::Prefix(_),
                    )
                });
        if escapes {
            return Err(self.failed(format!("patch path escapes target dir: {}", rel.display())));
        }
        self.ensure_parents_within_dir(rel)?;
        Ok(self.patched_dir.join(rel))
    }

    fn ensure_parents_within_dir(&self, rel: &Path) -> Result<(), PatchApplyError> {
        let Some(parent) = rel.parent() else {
            return Ok(());
        };
        let mut current = self.canonical_dir.to_path_buf();
        for component in parent.components() {
            current.push(component);
            self.check_ancestor_symlink(&mut current, rel)?;
        }
        Ok(())
    }

    fn check_ancestor_symlink(
        &self,
        current: &mut PathBuf,
        rel: &Path,
    ) -> Result<(), PatchApplyError> {
        if current.symlink_metadata().is_err() {
            return Ok(());
        }
        let canonical = fs::canonicalize(&current)
            .map_err(|source| {
                self.failed(format!("canonicalize {}: {source}", current.display()))
            })?;
        if !canonical.starts_with(self.canonical_dir) {
            return Err(self.failed(format!(
                "patch path escapes target dir via symlink: {}",
                rel.display(),
            )));
        }
        *current = canonical;
        Ok(())
    }

    fn modify(
        &self,
        target: &Path,
        text_patch: &Patch<'_, str>,
        new_mode: Option<&FileMode>,
    ) -> Result<(), PatchApplyError> {
        let permissions = atomic_write::modify_permissions(target, new_mode)
            .map_err(|source| self.failed(format!("stat {}: {source}", target.display())))?;
        let bytes = fs::read(target)
            .map_err(|source| self.failed(format!("read {}: {source}", target.display())))?;
        let original = String::from_utf8_lossy(&bytes).into_owned();
        let updated = match tolerant::apply(&original, text_patch) {
            Ok(updated) => updated,
            // File is already in the post-patch state — reverse applies
            // cleanly. If the patch requested a mode change that is not yet
            // reflected, or the target is still a symlink, re-write atomically
            // to break store hardlinks and safely isolate without mutating inodes in-place.
            Err(_) if tolerant::apply(&original, &text_patch.reverse()).is_ok() => {
                if atomic_write::needs_mode_change(target, new_mode) || target.is_symlink() {
                    original
                } else {
                    return Ok(());
                }
            }
            Err(message) => {
                return Err(self.failed(format!("apply to {}: {message}", target.display())));
            }
        };
        // `rename` creates a new dirent → inode mapping at `target`, which
        // breaks the hardlink to the store. Files in
        // `node_modules/.pnpm/<slot>/node_modules/<pkg>` are
        // hardlinked (or reflinked) from the content-
        // addressable store; a plain truncating `fs::write`
        // would mutate the shared inode, corrupting the store
        // copy and every other snapshot's hardlink to it. The
        // patched output is captured by the side-effects cache
        // after this returns; nothing requires the store copy
        // to carry it.
        write_atomic_with_mode(target, updated.as_bytes(), Some(&permissions))
            .map_err(|source| self.failed(format!("write {}: {source}", target.display())))
    }

    /// A "new file" patch (`--- /dev/null`) means the target is expected NOT
    /// to exist. Refusing to overwrite matches `patch`'s and `git apply`'s
    /// behavior — silently clobbering a real file would be a data-loss
    /// footgun if the patch was authored against the wrong base.
    ///
    /// Idempotency exception: if the target already contains exactly the
    /// post-patch content, the patch has already been applied (e.g. a
    /// re-run) and this is a no-op.
    fn create(
        &self,
        target: &Path,
        text_patch: &Patch<'_, str>,
        new_mode: Option<&FileMode>,
    ) -> Result<(), PatchApplyError> {
        let created = tolerant::apply("", text_patch)
            .map_err(|message| self.failed(format!("create {}: {message}", target.display())))?;
        let exists = target.symlink_metadata().is_ok();
        if exists && self.existing_file_matches(target, &created, new_mode)? {
            return Ok(());
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|source| {
                    self.failed(format!("create parent of {}: {source}", target.display()))
                })?;
        }
        let permissions = atomic_write::create_permissions(new_mode);
        write_atomic_with_mode(target, created.as_bytes(), permissions.as_ref())
            .map_err(|source| self.failed(format!("write {}: {source}", target.display())))
    }

    fn existing_file_matches(
        &self,
        target: &Path,
        created: &str,
        new_mode: Option<&FileMode>,
    ) -> Result<bool, PatchApplyError> {
        if target.is_symlink() {
            let target = target.display();
            return Err(self.failed(format!("cannot create {target}: target already exists")));
        }
        let existing = fs::read(target)
            .map_err(|source| self.failed(format!("read {}: {source}", target.display())))?;
        if String::from_utf8_lossy(&existing) != created {
            let target = target.display();
            return Err(self.failed(format!("cannot create {target}: target already exists")));
        }
        Ok(!atomic_write::needs_mode_change(target, new_mode))
    }

    fn delete(&self, target: &Path, text_patch: &Patch<'_, str>) -> Result<(), PatchApplyError> {
        self.check_delete_preimage(target, text_patch)?;
        match fs::remove_file(target) {
            Ok(()) => Ok(()),
            // A missing target means the file was already removed by an
            // earlier apply of the same patch.
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(self.failed(format!("delete {}: {source}", target.display()))),
        }
    }

    /// A delete block that carries hunks is validated against the file on
    /// disk before unlinking — a stale or wrong-target patch would otherwise
    /// silently delete the wrong file. `diffy::apply` on such a patch
    /// produces the empty string when every hunk matches.
    ///
    /// `git diff --irreversible-delete`, which `pnpm patch` and
    /// `pnpm patch-commit` run, writes the header of a deleted file without
    /// its preimage. There are no hunks to check then, so the file is
    /// unlinked on the header alone, as pnpm 11 does for every deletion.
    fn check_delete_preimage(
        &self,
        target: &Path,
        text_patch: &Patch<'_, str>,
    ) -> Result<(), PatchApplyError> {
        if text_patch.hunks().is_empty() {
            return Ok(());
        }
        // Lossy UTF-8 decoding for the same reason as `modify`: match the
        // patch-file reader and Node's `fs.readFile(..., 'utf8')`.
        let bytes = match fs::read(target) {
            Ok(bytes) => bytes,
            // Already removed by an earlier apply of the same patch.
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(self.failed(format!("read {}: {source}", target.display())));
            }
        };
        let original = String::from_utf8_lossy(&bytes).into_owned();
        let after = tolerant::apply(&original, text_patch)
            .map_err(|message| self.failed(format!("apply to {}: {message}", target.display())))?;
        if after.is_empty() {
            return Ok(());
        }
        Err(self.failed(format!(
            "delete patch left {} non-empty after apply ({} bytes remain)",
            target.display(),
            after.len(),
        )))
    }
}

#[cfg(test)]
mod tests;
