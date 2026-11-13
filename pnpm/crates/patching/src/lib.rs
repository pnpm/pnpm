//! Applying and committing the patch files of pnpm's `patchedDependencies`.
//!
//! - [`apply_patch_to_dir`] applies a unified-diff patch to an extracted
//!   package directory before postinstall hooks run.
//! - [`prepare_pkg_files_for_diff`] and [`diff_folders`] create the filtered
//!   package diff written by `pnpm patch-commit`.
//!
//! Parsing, grouping, matching, and hashing the entries live in
//! [`pnpm_patching_config`], which this crate re-exports.

pub(crate) use pnpm_process as process;

pub use apply::{
    MANIFEST_FILE_NAME, PatchApplyError, PatchPreview, apply_patch_to_dir, preview_patch,
};
pub use commit::*;
pub use pnpm_patching_config::{
    CalcPatchHashError, ExtendedPatchInfo, ParsedKey, PatchGroup, PatchGroupRangeItem,
    PatchGroupRecord, PatchInfo, PatchInput, PatchKeyConflictError, PatchNonSemverRangeError,
    ResolvePatchedDependenciesError, UnusedPatchError, UnusedPatches, all_patch_keys,
    calc_patch_hashes, create_hex_hash_from_file, get_patch_info, group_patched_dependencies,
    parse_key, resolve_and_group, verify_patches,
};

mod apply;
mod commit;
