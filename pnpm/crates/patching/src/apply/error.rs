//! Error type for [`apply_patch_to_dir`](super::apply_patch_to_dir).

use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use std::{
    io,
    path::PathBuf,
};

/// Error from [`apply_patch_to_dir`](super::apply_patch_to_dir).
///
/// Diagnostic codes:
///
/// - `ERR_PNPM_PATCH_NOT_FOUND`, the patch file is missing.
/// - `ERR_PNPM_INVALID_PATCH`, the patch file can't be parsed.
/// - `ERR_PNPM_PATCH_FAILED`, a hunk wouldn't apply, the target
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
