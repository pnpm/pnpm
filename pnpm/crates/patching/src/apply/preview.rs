use super::{PatchApplyError, read_patch_text, tolerant};

use diffy::patch_set::{FileOperation, FilePatch, ParseOptions, PatchSet};
use indexmap::IndexSet;
use std::{
    fs,
    path::{Component, Path},
};

/// How a package's manifest is spelled in [`PatchPreview::written_paths`] and
/// [`PatchPreview::removed_paths`].
pub const MANIFEST_FILE_NAME: &str = "package.json";

/// What a patch file would leave behind in a package directory, read
/// without writing anything.
///
/// pnpm decides what a package's build needs well before the build phase
/// applies the patch, and a patch can introduce build triggers of its
/// own. [`preview_patch`] lets those decisions see the patched package.
#[derive(Debug, Default)]
pub struct PatchPreview {
    /// The `package.json` the patch would leave, or `None` when it does
    /// not touch the manifest.
    pub manifest: Option<String>,
    /// The paths the patch creates or rewrites, relative to the package
    /// directory, `/`-separated and free of `.` segments. Deletions are
    /// left out: a file a patch removes is not one the package has.
    pub written_paths: Vec<String>,
    /// The paths the patch deletes, in the same spelling as
    /// [`Self::written_paths`]. A path the patch deletes and then writes
    /// again is reported as written rather than removed, because that is what
    /// the package is left holding.
    ///
    /// A caller deciding what the patched package holds needs these too: a
    /// file the package published survives the patch unless it is here.
    pub removed_paths: Vec<String>,
}

/// Read what `patch_file_path` would leave in `patched_dir`.
///
/// A manifest that already carries the patch reports its content as it
/// stands, for the same reason [`apply_patch_to_dir`] treats a re-apply
/// as a no-op.
///
/// [`apply_patch_to_dir`]: super::apply_patch_to_dir
pub fn preview_patch(
    patched_dir: &Path,
    patch_file_path: &Path,
) -> Result<PatchPreview, PatchApplyError> {
    let text = read_patch_text(patch_file_path)?;
    let mut state = PreviewState {
        patched_dir,
        patch_file_path,
        preview: PatchPreview::default(),
        // Written paths are tracked as a set so a delete record costs one
        // lookup rather than a scan of everything written so far, and
        // insertion order is kept because a patch that rewrites a file twice
        // should not report it twice.
        written_paths: IndexSet::new(),
        // Tracked as a set for the same reasons, and kept disjoint from
        // `written_paths`: whichever record came last decides which side a
        // path ends up on.
        removed_paths: IndexSet::new(),
        // A patch may delete the manifest and write a new one, so "no
        // manifest record yet" and "the manifest is gone" are different
        // states.
        manifest_removed: false,
    };
    for file_patch_result in PatchSet::parse(&text, ParseOptions::gitdiff()) {
        let file_patch = file_patch_result.map_err(|source| PatchApplyError::InvalidPatch {
            patch_file_path: patch_file_path.to_path_buf(),
            message: source.to_string(),
        })?;
        state.record(&file_patch)?;
    }
    let PreviewState {
        mut preview,
        written_paths,
        removed_paths,
        ..
    } = state;
    preview.written_paths = written_paths.into_iter().collect();
    preview.removed_paths = removed_paths.into_iter().collect();
    Ok(preview)
}

/// What the records read so far would leave behind.
struct PreviewState<'a> {
    patched_dir: &'a Path,
    patch_file_path: &'a Path,
    preview: PatchPreview,
    written_paths: IndexSet<String>,
    removed_paths: IndexSet<String>,
    manifest_removed: bool,
}

impl PreviewState<'_> {
    /// Fold one file record into the preview.
    fn record(&mut self, file_patch: &FilePatch<'_, str>) -> Result<(), PatchApplyError> {
        let operation = file_patch.operation().strip_prefix(1);
        let raw_path = match &operation {
            FileOperation::Modify { modified, .. } | FileOperation::Create(modified) => {
                modified.as_ref()
            }
            FileOperation::Delete(path) => {
                self.remove(path.as_ref());
                return Ok(());
            }
            _ => return Ok(()),
        };
        let Some(written) = normalized_patch_path(raw_path) else { return Ok(()) };
        let is_manifest = names_manifest(self.patched_dir, &written);
        self.removed_paths.shift_remove(&written);
        self.written_paths.insert(written);
        if !is_manifest {
            return Ok(());
        }
        // A package ships a manifest, so a `Create` naming one only makes
        // sense once an earlier record removed it; `apply_patch_to_dir`
        // reports every other spelling. A `Modify` of a removed manifest is
        // left to it for the same reason.
        if matches!(operation, FileOperation::Create(_)) != self.manifest_removed {
            return Ok(());
        }
        self.apply_to_manifest(file_patch)
    }

    /// A patch that writes a file and then removes it leaves the package
    /// without it, so an earlier record's path is dropped rather than merely
    /// skipped.
    fn remove(&mut self, path: &str) {
        let Some(removed) = normalized_patch_path(path) else { return };
        if names_manifest(self.patched_dir, &removed) {
            self.preview.manifest = None;
            self.manifest_removed = true;
        }
        self.written_paths.shift_remove(&removed);
        self.removed_paths.insert(removed);
    }

    fn apply_to_manifest(
        &mut self,
        file_patch: &FilePatch<'_, str>,
    ) -> Result<(), PatchApplyError> {
        let target = self.patched_dir.join(MANIFEST_FILE_NAME);
        let original = self.manifest_before(&target)?;
        self.manifest_removed = false;
        let text_patch = file_patch
            .patch()
            .as_text()
            .ok_or_else(|| self.failed("binary patch is not supported".to_string()))?;
        self.preview.manifest = Some(match tolerant::apply(&original, text_patch) {
            Ok(updated) => updated,
            Err(_) if tolerant::apply(&original, &text_patch.reverse()).is_ok() => original,
            Err(message) => {
                return Err(self.failed(format!("apply to {}: {message}", target.display())));
            }
        });
        Ok(())
    }

    /// The manifest this record patches. A patch may carry more than one
    /// record for the same file, and [`apply_patch_to_dir`] feeds each the
    /// previous one's output; they are chained here too, or the preview
    /// would answer for the last record alone.
    ///
    /// [`apply_patch_to_dir`]: super::apply_patch_to_dir
    fn manifest_before(&mut self, target: &Path) -> Result<String, PatchApplyError> {
        if let Some(patched_so_far) = self.preview.manifest.take() {
            return Ok(patched_so_far);
        }
        if self.manifest_removed {
            return Ok(String::new());
        }
        let bytes = fs::read(target)
            .map_err(|source| self.failed(format!("read {}: {source}", target.display())))?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn failed(&self, message: String) -> PatchApplyError {
        PatchApplyError::PatchFailed {
            patch_file_path: self.patch_file_path.to_path_buf(),
            patched_dir: self.patched_dir.to_path_buf(),
            message,
        }
    }
}

/// Whether `written` names the package's manifest.
///
/// A patch header may spell it in another case, and on a case-insensitive
/// volume the applier still reaches the real `package.json`. The filesystem
/// is asked rather than the platform guessed: where names are
/// case-sensitive, `Package.json` is a different file and must not be
/// mistaken for the manifest. The check costs nothing for the spelling
/// every patch actually uses.
fn names_manifest(patched_dir: &Path, written: &str) -> bool {
    if written == MANIFEST_FILE_NAME {
        return true;
    }
    if !written.eq_ignore_ascii_case(MANIFEST_FILE_NAME) {
        return false;
    }
    let (Ok(spelled), Ok(manifest)) = (
        fs::canonicalize(patched_dir.join(written)),
        fs::canonicalize(patched_dir.join(MANIFEST_FILE_NAME)),
    ) else {
        return false;
    };
    spelled == manifest
}

/// The path a patch header names, spelled the way [`apply_patch_to_dir`]
/// resolves it: `.` segments dropped, separators normalized to `/`.
///
/// `None` for a path that call would refuse — one that is absolute or
/// climbs out of the package — so the preview never reports as written a
/// path the apply will reject.
///
/// [`apply_patch_to_dir`]: super::apply_patch_to_dir
fn normalized_patch_path(rel: &str) -> Option<String> {
    let mut segments = Vec::new();
    for component in Path::new(rel).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => segments.push(segment.to_string_lossy().into_owned()),
            _ => return None,
        }
    }
    (!segments.is_empty()).then(|| segments.join("/"))
}
