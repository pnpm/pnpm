pub use error::PatchApplyError;
pub use preview::{
    MANIFEST_FILE_NAME,
    PatchPreview,
    preview_patch,
};

mod atomic_write;
mod binary;
mod error;
mod preview;
mod tolerant;

use atomic_write::write_atomic_with_mode;
use diffy::{
    Patch,
    patch_set::{
        FileMode,
        FileOperation,
        FilePatch,
        ParseOptions,
        PatchKind,
        PatchSet,
    },
};
use std::{
    fs,
    io,
    path::{
        Component,
        Path,
        PathBuf,
    },
};

/// Apply a unified-diff patch file to every modified/created/deleted
/// file inside `patched_dir`.
///
/// Uses [`diffy`] for parsing and applying, pure Rust, no subprocess,
/// no Node, cross-platform. Running `patch` or `git apply` directly
/// would be simpler, but `patch` is not available on Windows and
/// `git apply` is hard to execute on a subdirectory of an existing
/// repository, so an in-process applier sidesteps both problems.
///
/// Supported file operations: `Modify`, `Create`, `Delete`, `Rename`.
///
/// File paths in the patch are read with the `a/` and `b/` prefixes git
/// uses dropped, then validated against
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

    let apply = FileApply {
        patched_dir,
        canonical_dir: &canonical_dir,
        patch_file_path,
        patch_text: &text,
    };
    for file_patch in parse_file_patches(&text, patch_file_path) {
        apply.apply_one_file(&file_patch?)?;
    }
    Ok(())
}

/// Parse the records of a patch file read by [`read_patch_text`].
///
/// A patch without `diff --git` lines is read as a plain unified diff, as
/// `diff -u` and old patch-package versions write it.
///
/// A record that creates or changes a symlink or a submodule is rejected:
/// only regular and executable files can be patched.
pub(super) fn parse_file_patches<'a>(
    text: &'a str,
    patch_file_path: &'a Path,
) -> impl Iterator<Item = Result<FilePatch<'a, str>, PatchApplyError>> + 'a {
    let has_git_headers = text
        .lines()
        .any(|line| line.starts_with("diff --git "));
    let (records, options) = if has_git_headers {
        (vec![text], ParseOptions::gitdiff())
    } else {
        (tolerant::unidiff_records(text), ParseOptions::unidiff())
    };
    let invalid = move |message: String| PatchApplyError::InvalidPatch {
        patch_file_path: patch_file_path.to_path_buf(),
        message,
    };
    records
        .into_iter()
        .flat_map(move |record| PatchSet::parse(record, options.clone()))
        .map(move |file_patch_result| {
            let file_patch =
                file_patch_result.map_err(|source| source.to_string()).map_err(invalid)?;
            let unsupported_mode = [file_patch.old_mode(), file_patch.new_mode()]
                .into_iter()
                .flatten()
                .find_map(|mode| unsupported_file_mode(*mode));
            match unsupported_mode {
                Some(mode) => Err(invalid(format!("Unexpected file mode string: {mode}"))),
                None => Ok(file_patch),
            }
        })
}

fn unsupported_file_mode(mode: FileMode) -> Option<&'static str> {
    match mode {
        FileMode::Regular | FileMode::Executable => None,
        FileMode::Symlink => Some("120000"),
        FileMode::Gitlink => Some("160000"),
    }
}

/// Read a patch file and apply the tolerances that let the patch files
/// pnpm meets in the wild through [`PatchSet::parse`].
///
/// [`tolerant`] owns the tolerances; this is only the order it expects
/// them in, kept in one place so the apply and preview paths cannot drift.
pub(super) fn read_patch_text(patch_file_path: &Path) -> Result<String, PatchApplyError> {
    let text = read_patch_file(patch_file_path)?;
    let text = tolerant::strip_cr_from_extended_headers(text);
    Ok(tolerant::drop_context_no_newline_markers(tolerant::mark_blank_crlf_context_lines(text)))
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

impl FileApply<'_> {
    fn apply_one_file(&self, file_patch: &FilePatch<'_, str>) -> Result<(), PatchApplyError> {
        let operation = package_relative_operation(file_patch);
        match file_patch.patch() {
            PatchKind::Binary(binary_patch) => {
                self.binary_operation(&operation, binary_patch, file_patch.new_mode())
            }
            PatchKind::Text(text_patch) => {
                self.text_operation(&operation, text_patch, file_patch.new_mode())
            }
        }
    }

    /// Apply one text record, dispatching on the operation.
    fn text_operation(
        &self,
        operation: &FileOperation<'_, str>,
        text_patch: &Patch<'_, str>,
        new_mode: Option<&FileMode>,
    ) -> Result<(), PatchApplyError> {
        match operation {
            FileOperation::Modify { modified, .. } => self.modify(
                &self.resolve_target(Path::new(modified.as_ref()))?,
                text_patch,
                new_mode,
            ),
            FileOperation::Create(path) => {
                self.create(&self.resolve_target(Path::new(path.as_ref()))?, text_patch, new_mode)
            }
            FileOperation::Delete(path) => {
                self.delete(&self.resolve_target(Path::new(path.as_ref()))?, text_patch)
            }
            FileOperation::Rename { from, to } => self.rename(
                &self.resolve_target(Path::new(from.as_ref()))?,
                &self.resolve_target(Path::new(to.as_ref()))?,
                text_patch,
                new_mode,
            ),
            FileOperation::Copy { .. } => {
                Err(self.failed("copy operations in patches are not yet supported".to_string()))
            }
        }
    }
}

/// The record's operation with paths relative to the patched directory.
///
/// Git prefixes the `diff --git`, `---` and `+++` paths with `a/` and `b/`,
/// which are stripped, but writes `rename from` and `rename to` bare.
pub(super) fn package_relative_operation<'patch>(
    file_patch: &'patch FilePatch<'_, str>,
) -> FileOperation<'patch, str> {
    match file_patch.operation() {
        rename @ FileOperation::Rename { .. } => rename.clone(),
        operation => operation.strip_prefix(1),
    }
}

/// One patch record being applied to one file of `patched_dir`.
pub(super) struct FileApply<'a> {
    patched_dir: &'a Path,
    canonical_dir: &'a Path,
    patch_file_path: &'a Path,
    /// The text the records were parsed from.
    patch_text: &'a str,
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

    /// A record without hunks only changes the mode, so the file's bytes are
    /// written back undecoded: a binary file must not pass through UTF-8.
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
        let updated = if text_patch.hunks().is_empty() {
            bytes
        } else {
            match self.patched_text(target, &bytes, text_patch, new_mode)? {
                Some(updated) => updated.into_bytes(),
                None => return Ok(()),
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
        write_atomic_with_mode(target, &updated, Some(&permissions))
            .map_err(|source| self.failed(format!("write {}: {source}", target.display())))
    }

    /// The file's content with the hunks applied, or `None` when the file
    /// already carries them and needs no rewrite.
    fn patched_text(
        &self,
        target: &Path,
        bytes: &[u8],
        text_patch: &Patch<'_, str>,
        new_mode: Option<&FileMode>,
    ) -> Result<Option<String>, PatchApplyError> {
        let original = String::from_utf8_lossy(bytes).into_owned();
        match tolerant::apply(&original, text_patch) {
            Ok(updated) => Ok(Some(updated)),
            // File is already in the post-patch state, reverse applies
            // cleanly. If the patch requested a mode change that is not yet
            // reflected, or the target is still a symlink, re-write atomically
            // to break store hardlinks and safely isolate without mutating inodes in-place.
            Err(_) if tolerant::apply(&original, &text_patch.reverse()).is_ok() => {
                let rewrite =
                    atomic_write::needs_mode_change(target, new_mode) || target.is_symlink();
                Ok(rewrite.then_some(original))
            }
            Err(message) => Err(self.failed(format!("apply to {}: {message}", target.display()))),
        }
    }

    /// An existing `to` is not overwritten, for the reason [`Self::create`]
    /// gives. A missing `from` means an earlier apply of the same patch
    /// already moved it. A rename without hunks or a mode change only moves
    /// the entry, so its bytes are never decoded and a symlink stays a
    /// symlink rather than being replaced by what it points at.
    fn rename(
        &self,
        from: &Path,
        to: &Path,
        text_patch: &Patch<'_, str>,
        new_mode: Option<&FileMode>,
    ) -> Result<(), PatchApplyError> {
        self.move_entry(from, to)?;
        if text_patch.hunks().is_empty() && !atomic_write::needs_mode_change(to, new_mode) {
            return Ok(());
        }
        self.modify(to, text_patch, new_mode)
    }

    /// Move `from` to `to` for a rename record: refuse to overwrite an
    /// existing `to` (see [`Self::create`]), tolerate a missing `from` when
    /// an earlier apply of the same patch already moved it, and leave a
    /// missing pair to the caller's error handling.
    fn move_entry(&self, from: &Path, to: &Path) -> Result<(), PatchApplyError> {
        if from.symlink_metadata().is_ok() {
            if to.symlink_metadata().is_ok() {
                let (from, to) = (from.display(), to.display());
                return Err(self.failed(format!(
                    "cannot rename {from} to {to}: target already exists",
                )));
            }
            self.create_parent_dir(to)?;
            fs::rename(from, to)
                .map_err(|source| {
                    self.failed(format!("rename {} to {}: {source}", from.display(), to.display()))
                })?;
        } else if to.symlink_metadata().is_err() {
            let (from, to) = (from.display(), to.display());
            return Err(self.failed(format!("cannot rename {from} to {to}: neither exists")));
        }
        Ok(())
    }

    /// A "new file" patch (`--- /dev/null`) means the target is expected NOT
    /// to exist. Refusing to overwrite matches `patch`'s and `git apply`'s
    /// behavior, silently clobbering a real file would be a data-loss
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
        if exists && self.existing_file_matches(target, created.as_bytes(), new_mode)? {
            return Ok(());
        }
        self.create_parent_dir(target)?;
        let permissions = atomic_write::create_permissions(new_mode);
        write_atomic_with_mode(target, created.as_bytes(), permissions.as_ref())
            .map_err(|source| self.failed(format!("write {}: {source}", target.display())))
    }

    fn create_parent_dir(&self, target: &Path) -> Result<(), PatchApplyError> {
        let Some(parent) = target.parent() else { return Ok(()) };
        fs::create_dir_all(parent)
            .map_err(|source| {
                self.failed(format!("create parent of {}: {source}", target.display()))
            })
    }

    fn existing_file_matches(
        &self,
        target: &Path,
        created: &[u8],
        new_mode: Option<&FileMode>,
    ) -> Result<bool, PatchApplyError> {
        if target.is_symlink() {
            let target = target.display();
            return Err(self.failed(format!("cannot create {target}: target already exists")));
        }
        let existing = fs::read(target)
            .map_err(|source| self.failed(format!("read {}: {source}", target.display())))?;
        if existing != created {
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
    /// disk before unlinking, a stale or wrong-target patch would otherwise
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
