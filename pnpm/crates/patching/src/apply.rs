mod tolerant;

use derive_more::{Display, Error};
use diffy::{
    Patch,
    patch_set::{FileOperation, FilePatch, ParseOptions, PatchSet},
};
use indexmap::IndexSet;
use miette::Diagnostic;
use std::{
    fs::{self, OpenOptions, Permissions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Error from [`apply_patch_to_dir`].
///
/// Surfaces three diagnostic codes:
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
/// `ERR_PNPM_PATCH_FAILED`. A patch file is attacker-controlled
/// data — an `a/../../outside` header would otherwise let it
/// read, write, or delete outside the package directory.
///
/// `Modify` writes the patched content via a sibling temp file +
/// `rename` (the same pattern
/// [`pnpm_lockfile::save_lockfile::write_atomic`](../../lockfile/src/save_lockfile.rs)
/// uses for the lockfile), which both makes the rewrite crash-safe
/// — a failed write leaves the original on disk instead of an empty
/// dirent — and breaks any hardlink (or reflink) back to the
/// content-addressable store as a side effect of the rename creating
/// a new dirent → inode mapping. A plain truncating write would
/// otherwise corrupt the store copy that every other snapshot of the
/// same package shares, and leak patched content into sibling
/// snapshots. The patched output lives in the side-effects cache
/// after this call returns; nothing requires the store copy to carry
/// it. The temp file is chmoded to match the original before rename so
/// patched shebang scripts in `bin/` keep their executable bit
/// atomically — no window where the file has the wrong mode.
pub fn apply_patch_to_dir(
    patched_dir: &Path,
    patch_file_path: &Path,
) -> Result<(), PatchApplyError> {
    let text = read_patch_file(patch_file_path)?;

    let patches = PatchSet::parse(&text, ParseOptions::gitdiff());
    for file_patch_result in patches {
        let file_patch = file_patch_result.map_err(|source| PatchApplyError::InvalidPatch {
            patch_file_path: patch_file_path.to_path_buf(),
            message: source.to_string(),
        })?;
        apply_one_file(patched_dir, patch_file_path, &file_patch)?;
    }
    Ok(())
}

const MANIFEST_FILE_NAME: &str = "package.json";

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
}

/// Read what `patch_file_path` would leave in `patched_dir`.
///
/// A manifest that already carries the patch reports its content as it
/// stands, for the same reason [`apply_patch_to_dir`] treats a re-apply
/// as a no-op.
pub fn preview_patch(
    patched_dir: &Path,
    patch_file_path: &Path,
) -> Result<PatchPreview, PatchApplyError> {
    let text = read_patch_file(patch_file_path)?;
    let mut state = PreviewState {
        patched_dir,
        patch_file_path,
        preview: PatchPreview::default(),
        // Written paths are tracked as a set so a delete record costs one
        // lookup rather than a scan of everything written so far, and
        // insertion order is kept because a patch that rewrites a file twice
        // should not report it twice.
        written_paths: IndexSet::new(),
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
    let PreviewState { mut preview, written_paths, .. } = state;
    preview.written_paths = written_paths.into_iter().collect();
    Ok(preview)
}

/// What the records read so far would leave behind.
struct PreviewState<'a> {
    patched_dir: &'a Path,
    patch_file_path: &'a Path,
    preview: PatchPreview,
    written_paths: IndexSet<String>,
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
    patch_file_path: &Path,
    file_patch: &FilePatch<'_, str>,
) -> Result<(), PatchApplyError> {
    // Strip the conventional `a/` / `b/` prefix so the path inside
    // the patch maps onto a relative path under `patched_dir`.
    let operation = file_patch.operation().strip_prefix(1);
    let apply = FileApply { patched_dir, patch_file_path };

    let text_patch = || {
        file_patch
            .patch()
            .as_text()
            .ok_or_else(|| apply.failed("binary patch is not supported".to_string()))
    };
    match operation {
        FileOperation::Modify { modified, .. } => {
            apply.modify(&apply.resolve_target(Path::new(modified.as_ref()))?, text_patch()?)
        }
        FileOperation::Create(path) => {
            apply.create(&apply.resolve_target(Path::new(path.as_ref()))?, text_patch()?)
        }
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
    /// `..` segments, and (on Windows) drive-letter prefixes and root
    /// components. A patch is attacker-controlled data — an
    /// `a/../../outside` header could otherwise read, write, or delete files
    /// outside the package directory.
    fn resolve_target(&self, rel: &Path) -> Result<PathBuf, PatchApplyError> {
        let escapes = rel.is_absolute()
            || rel.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_),
                )
            });
        if escapes {
            return Err(self.failed(format!("patch path escapes target dir: {}", rel.display())));
        }
        Ok(self.patched_dir.join(rel))
    }

    fn modify(&self, target: &Path, text_patch: &Patch<'_, str>) -> Result<(), PatchApplyError> {
        // Capture the original mode so the rewritten file keeps
        // it. `fs::write` after `fs::remove_file` creates a fresh
        // inode whose mode is governed by the process umask, which
        // would otherwise drop the executable bit on patched
        // shebang scripts in `bin/`.
        let permissions = fs::metadata(target)
            .map(|metadata| metadata.permissions())
            .map_err(|source| self.failed(format!("stat {}: {source}", target.display())))?;
        // Read as bytes and lossy-decode so non-UTF-8 bytes
        // turn into U+FFFD rather than failing the patch.
        // Matches how the patch file itself is read (see
        // [`apply_patch_to_dir`]) and Node `fs.readFile(..., 'utf8')`.
        let bytes = fs::read(target)
            .map_err(|source| self.failed(format!("read {}: {source}", target.display())))?;
        let original = String::from_utf8_lossy(&bytes).into_owned();
        let updated = match tolerant::apply(&original, text_patch) {
            Ok(updated) => updated,
            // File is already in the post-patch state — reverse applies
            // cleanly, so treat as no-op.
            Err(_) if tolerant::apply(&original, &text_patch.reverse()).is_ok() => return Ok(()),
            Err(message) => {
                return Err(self.failed(format!("apply to {}: {message}", target.display())));
            }
        };
        // Stage the patched bytes in a sibling temp file, then
        // atomically rename over the target. `rename` creates a new
        // dirent → inode mapping at `target`, which both:
        //
        //   1. **Breaks the hardlink to the store.** Files in
        //      `node_modules/.pnpm/<slot>/node_modules/<pkg>` are
        //      hardlinked (or reflinked) from the content-
        //      addressable store; a plain truncating `fs::write`
        //      would mutate the shared inode, corrupting the store
        //      copy and every other snapshot's hardlink to it. The
        //      patched output is captured by the side-effects cache
        //      after this returns; nothing requires the store copy
        //      to carry it.
        //   2. **Is crash-safe.** If the write fails after we've
        //      unlinked the target, the package is broken until
        //      reinstall — `unlink → write` would have that
        //      window. With temp + rename, a mid-write failure
        //      just leaves a stale temp file (cleaned up best-
        //      effort) and the original target intact, so the next
        //      install can retry from the same baseline.
        write_atomic_with_mode(target, updated.as_bytes(), &permissions)
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
    fn create(&self, target: &Path, text_patch: &Patch<'_, str>) -> Result<(), PatchApplyError> {
        let created = tolerant::apply("", text_patch)
            .map_err(|message| self.failed(format!("create {}: {message}", target.display())))?;
        if target.try_exists().unwrap_or(false) {
            let existing = fs::read(target)
                .map_err(|source| self.failed(format!("read {}: {source}", target.display())))?;
            if String::from_utf8_lossy(&existing) == created {
                return Ok(());
            }
            let target = target.display();
            return Err(self.failed(format!("cannot create {target}: target already exists")));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                self.failed(format!("create parent of {}: {source}", target.display()))
            })?;
        }
        fs::write(target, created)
            .map_err(|source| self.failed(format!("write {}: {source}", target.display())))
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

/// Atomic write: stage `content` in a sibling temp file (with
/// `permissions` applied before the rename so the final file has the
/// right mode atomically), then `rename` over `target`. Mirrors the
/// pattern in
/// [`pnpm_lockfile::save_lockfile::write_atomic`](../../lockfile/src/save_lockfile.rs):
/// `create_new(true)` rather than `create + truncate` so we never
/// follow a symlink or truncate a file an attacker (or a crashed prior
/// install) pre-seeded at our predicted temp path; on `AlreadyExists`
/// the counter advances and we retry up to `MAX_TEMP_ATTEMPTS` times.
///
/// `rename` is atomic on Unix and replaces in-place on Windows, so an
/// IO failure mid-write leaves either the original file or the
/// rewritten one — never an empty dirent. **This is atomic against IO
/// errors, not against power loss**: we don't `fsync` the temp file
/// or the parent directory, so a host crash between rename and the
/// kernel's writeback flush can lose the rename. This matches Node's
/// `fs.writeFileSync` semantics — it doesn't fsync either, and a
/// partially-written patched install is recoverable by re-running
/// `pnpm install` anyway.
///
/// As a side effect, `rename` creates a fresh inode at `target`,
/// breaking any hardlink the path previously shared with the content-
/// addressable store; the store inode (and every other hardlink to it)
/// stays untouched.
fn write_atomic_with_mode(
    target: &Path,
    content: &[u8],
    permissions: &Permissions,
) -> io::Result<()> {
    /// Sixteen fresh counter values is plenty — under benign
    /// conditions we never collide; under shared-store-across-
    /// containers the chance of 16 consecutive same-pid same-counter
    /// collisions is negligible. Matches the constant in
    /// `pnpm_lockfile::save_lockfile::write_atomic`.
    const MAX_TEMP_ATTEMPTS: usize = 16;

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let pid = std::process::id();
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let file_name = target
        .file_name()
        .map_or_else(|| String::from("patched"), |name| name.to_string_lossy().into_owned());

    let mut last_already_exists: Option<io::Error> = None;
    for _ in 0..MAX_TEMP_ATTEMPTS {
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = parent.join(format!(".{file_name}.{pid}.{counter}.pacquet-tmp"));

        let mut file = match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                last_already_exists = Some(error);
                continue;
            }
            Err(error) => return Err(error),
        };

        if let Err(error) = file.write_all(content) {
            drop(file);
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }
        // Close before chmod / rename. Required on Windows: `MoveFileEx`
        // over a still-open source handle fails with a sharing
        // violation. Not strictly required on Unix but matches the
        // pattern in `save_lockfile::write_atomic`. No `sync_all`: this
        // routine is atomic against IO errors, not power loss — see
        // the `fn` doc above.
        drop(file);

        if let Err(error) = fs::set_permissions(&tmp, permissions.clone()) {
            let _ = fs::remove_file(&tmp);
            return Err(error);
        }

        return fs::rename(&tmp, target).inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        });
    }

    Err(last_already_exists.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "exhausted temp-path attempts for atomic patch write",
        )
    }))
}

#[cfg(test)]
mod tests;
