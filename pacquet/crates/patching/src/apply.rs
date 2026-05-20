use derive_more::{Display, Error};
use diffy::patch_set::{FileOperation, ParseOptions, PatchSet};
use miette::Diagnostic;
use std::path::{Component, Path, PathBuf};
use std::{fs, io};

/// Error from [`apply_patch_to_dir`].
///
/// Mirrors the three diagnostic codes upstream's
/// [`applyPatchToDir`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/apply-patch/src/index.ts)
/// surfaces:
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
/// Ports upstream's
/// [`applyPatchToDir`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/patching/apply-patch/src/index.ts),
/// which delegates to `@pnpm/patch-package`'s `applyPatch`. Pacquet
/// uses [`diffy`] for parsing and applying — pure Rust, no
/// subprocess, no Node, cross-platform. The upstream comment notes
/// "Ideally, we would just run `patch` or `git apply`. However,
/// `patch` is not available on Windows and `git apply` is hard to
/// execute on a subdirectory of an existing repository", which is
/// why pnpm vendored `patch-package`; pacquet sidesteps the same
/// problem the same way (in-process applier, no subprocess).
///
/// Supported file operations: `Modify`, `Create`, `Delete`.
/// `Rename`/`Copy` operations are reported as `ERR_PNPM_PATCH_FAILED`
/// with a descriptive message — they don't appear in
/// `patch-package`-style patches in practice, and the failure mode
/// is at least loud rather than silent.
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
/// `Create` refuses to overwrite an existing file (matches `patch`
/// and `git apply` semantics for `--- /dev/null` hunks) — unless the
/// file already contains exactly the post-patch content, in which
/// case the operation is treated as already applied. `Delete`
/// validates the hunks via `diffy::apply` and only unlinks when the
/// result is empty — a stale patch would otherwise silently delete
/// a file whose contents diverged from what the patch expects. A
/// missing target on `Delete` is treated as already applied.
///
/// `Modify` unlinks the target before writing the patched content,
/// breaking any hardlink (or reflink) back to the content-addressable
/// store. A plain truncating write would otherwise corrupt the store
/// copy that every other snapshot of the same package shares, and
/// leak patched content into sibling snapshots. The patched output
/// lives in the side-effects cache after this call returns; nothing
/// requires the store copy to carry it. The original mode is restored
/// after the rewrite so patched shebang scripts in `bin/` keep their
/// executable bit.
///
/// Apply is **idempotent**: when forward apply fails, the patch is
/// reverse-applied against the on-disk content. If the reverse
/// succeeds, the file is already in the post-patch state and the
/// hunk is treated as no-op. Mirrors upstream
/// [`@pnpm/patch-package`'s `applyPatch`](https://github.com/ds300/patch-package/blob/master/src/applyPatches.ts),
/// which on failure retries `executeEffects(reversePatch(patch), { dryRun: true })`
/// and returns success when the reverse cleanly verifies. Defense in
/// depth against re-runs that find the directory pre-patched (a side-
/// effects cache hit that fell through, manual edits, partial install
/// recovery): the hardlink-break above prevents fresh installs from
/// ever producing this state in the first place.
pub fn apply_patch_to_dir(
    patched_dir: &Path,
    patch_file_path: &Path,
) -> Result<(), PatchApplyError> {
    // Read the patch file. ENOENT becomes `ERR_PNPM_PATCH_NOT_FOUND`
    // (mirrors upstream's `if (err.code === 'ENOENT') throw new
    // PnpmError('PATCH_NOT_FOUND', ...)`). Every other IO error
    // surfaces with the same diagnostic code but a different
    // variant so the underlying `io::Error` chain is preserved.
    let bytes = match fs::read(patch_file_path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(PatchApplyError::PatchNotFound { path: patch_file_path.to_path_buf() });
        }
        Err(source) => {
            return Err(PatchApplyError::ReadPatchFile {
                path: patch_file_path.to_path_buf(),
                source,
            });
        }
    };
    // Lossy UTF-8 to match Node `fs.readFile(path, 'utf8')` (the
    // same decoding [`create_hex_hash_from_file`] uses), so a patch
    // file with stray bytes parses the same way upstream's reader
    // would see it.
    //
    // [`create_hex_hash_from_file`]: crate::create_hex_hash_from_file
    let text = String::from_utf8_lossy(&bytes);

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

fn apply_one_file(
    patched_dir: &Path,
    patch_file_path: &Path,
    file_patch: &diffy::patch_set::FilePatch<'_, str>,
) -> Result<(), PatchApplyError> {
    // Strip the conventional `a/` / `b/` prefix so the path inside
    // the patch maps onto a relative path under `patched_dir`.
    let operation = file_patch.operation().strip_prefix(1);

    let failed = |message: String| PatchApplyError::PatchFailed {
        patch_file_path: patch_file_path.to_path_buf(),
        patched_dir: patched_dir.to_path_buf(),
        message,
    };

    // Reject patch paths that try to escape `patched_dir`: absolute
    // paths, `..` segments, and (on Windows) drive-letter prefixes
    // and root components. A patch is attacker-controlled data —
    // an `a/../../outside` header could otherwise read, write, or
    // delete files outside the package directory.
    let resolve_target = |rel: &Path| -> Result<PathBuf, PatchApplyError> {
        if rel.is_absolute()
            || rel.components().any(|c| {
                matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_))
            })
        {
            return Err(failed(format!("patch path escapes target dir: {}", rel.display())));
        }
        Ok(patched_dir.join(rel))
    };

    match operation {
        FileOperation::Modify { modified, .. } => {
            let target = resolve_target(Path::new(modified.as_ref()))?;
            // Capture the original mode so the rewritten file keeps
            // it. `fs::write` after `fs::remove_file` creates a fresh
            // inode whose mode is governed by the process umask, which
            // would otherwise drop the executable bit on patched
            // shebang scripts in `bin/`. Mirrors upstream's
            // [`fs.writeFileSync(path, ..., { mode })`](https://github.com/ds300/patch-package/blob/master/src/patch/apply.ts).
            let permissions = fs::metadata(&target)
                .map(|m| m.permissions())
                .map_err(|source| failed(format!("stat {}: {source}", target.display())))?;
            // Read as bytes and lossy-decode so non-UTF-8 bytes
            // turn into U+FFFD rather than failing the patch.
            // Matches how the patch file itself is read (see
            // [`apply_patch_to_dir`]) and Node `fs.readFile(..., 'utf8')`,
            // which upstream's patch-package uses end-to-end.
            let bytes = fs::read(&target)
                .map_err(|source| failed(format!("read {}: {source}", target.display())))?;
            let original = String::from_utf8_lossy(&bytes).into_owned();
            let text_patch = file_patch
                .patch()
                .as_text()
                .ok_or_else(|| failed("binary patch is not supported".to_string()))?;
            let updated = match diffy::apply(&original, text_patch) {
                Ok(updated) => updated,
                Err(_) if diffy::apply(&original, &text_patch.reverse()).is_ok() => {
                    // File is already in the post-patch state — reverse
                    // applies cleanly, so treat as no-op.
                    return Ok(());
                }
                Err(source) => {
                    return Err(failed(format!("apply to {}: {source}", target.display())));
                }
            };
            // Break any hardlinks before writing. Files in
            // `node_modules/.pnpm/<slot>/node_modules/<pkg>` are
            // hardlinked (or reflinked, depending on import method) to
            // the content-addressable store; a plain `fs::write`
            // truncates and writes through the shared inode, which
            // would (a) corrupt the on-disk store copy and (b) leak
            // patched content into every other snapshot that linked
            // the same file. `remove_file` unlinks only this dirent —
            // the store inode (and any other hardlinks pointing at it)
            // stays untouched. The patched output is captured by the
            // side-effects cache after this returns; nothing requires
            // the store copy to carry it.
            fs::remove_file(&target)
                .map_err(|source| failed(format!("unlink {}: {source}", target.display())))?;
            fs::write(&target, updated)
                .map_err(|source| failed(format!("write {}: {source}", target.display())))?;
            fs::set_permissions(&target, permissions).map_err(|source| {
                failed(format!("restore mode of {}: {source}", target.display()))
            })?;
        }
        FileOperation::Create(path) => {
            let target = resolve_target(Path::new(path.as_ref()))?;
            let text_patch = file_patch
                .patch()
                .as_text()
                .ok_or_else(|| failed("binary patch is not supported".to_string()))?;
            let created = diffy::apply("", text_patch)
                .map_err(|source| failed(format!("create {}: {source}", target.display())))?;
            // A "new file" patch (`--- /dev/null`) means upstream
            // expects the target NOT to exist. Refusing to overwrite
            // matches `patch`'s and `git apply`'s behavior — silently
            // clobbering a real file would be a data-loss footgun if
            // the patch was authored against the wrong base.
            //
            // Idempotency exception: if the target already contains
            // exactly the post-patch content, the patch has already
            // been applied (e.g. a re-run) and we no-op. Matches
            // upstream's reverse-dry-run check in
            // `@pnpm/patch-package`'s `applyPatch`.
            if target.try_exists().unwrap_or(false) {
                let existing = fs::read(&target)
                    .map_err(|source| failed(format!("read {}: {source}", target.display())))?;
                if String::from_utf8_lossy(&existing) == created {
                    return Ok(());
                }
                return Err(failed(format!(
                    "cannot create {}: target already exists",
                    target.display(),
                )));
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|source| {
                    failed(format!("create parent of {}: {source}", target.display()))
                })?;
            }
            fs::write(&target, created)
                .map_err(|source| failed(format!("write {}: {source}", target.display())))?;
        }
        FileOperation::Delete(path) => {
            let target = resolve_target(Path::new(path.as_ref()))?;
            // Validate that the existing file matches the patch
            // before unlinking — a stale or wrong-target patch
            // would otherwise silently delete the wrong file.
            // diffy::apply on a delete patch produces the empty
            // string when every hunk matches.
            //
            // Lossy UTF-8 decoding for the same reason as the
            // `Modify` branch above: match the patch-file reader
            // and Node's `fs.readFile(..., 'utf8')`.
            //
            // Idempotency: a missing target means the file was
            // already removed by an earlier apply of the same
            // patch — treat as no-op. Matches upstream's
            // reverse-dry-run check in `@pnpm/patch-package`.
            let bytes = match fs::read(&target) {
                Ok(b) => b,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(source) => {
                    return Err(failed(format!("read {}: {source}", target.display())));
                }
            };
            let original = String::from_utf8_lossy(&bytes).into_owned();
            let text_patch = file_patch
                .patch()
                .as_text()
                .ok_or_else(|| failed("binary patch is not supported".to_string()))?;
            let after = diffy::apply(&original, text_patch)
                .map_err(|source| failed(format!("apply to {}: {source}", target.display())))?;
            if !after.is_empty() {
                return Err(failed(format!(
                    "delete patch left {} non-empty after apply ({} bytes remain)",
                    target.display(),
                    after.len(),
                )));
            }
            fs::remove_file(&target)
                .map_err(|source| failed(format!("delete {}: {source}", target.display())))?;
        }
        FileOperation::Rename { .. } | FileOperation::Copy { .. } => {
            return Err(failed(
                "rename/copy operations in patches are not yet supported".to_string(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
